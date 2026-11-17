# Spec: the session launcher

- Date: 2026-11-17
- Card: R4-04 (session launcher with adapters for Claude Code and Codex, and
  for a generic command)
- Status: Draft

## Problem

Release 4 gives a job to an agent. The job store (R4-01) holds the job, and
the workspace builder (R4-02) writes the folder of the job. Nothing starts an
agent in that folder, and nothing stops it. The workspace spec
(`docs/specs/2026-11-16-job-workspace.md`, "The build") gives the launcher
steps that keep the files and the grant in agreement, but no code does them.

The person uses more than one agent tool: Claude Code, Codex, or another
command. Each tool has its own command line.

## Goal

1. `iris job run <j-id> --agent <name>` starts one session of an agent in the
   workspace of the job. `iris job stop <j-id>` stops it.
2. The engine runs each session. A session of a job ends when the agent
   exits, when the person stops it, when the person revokes a part of the
   scope, when the job leaves the state `working`, at the time limit of the
   session, and when the engine stops.
3. When a session ends, no process of the session runs after the stop
   time (see [Stop](#stop)).
4. The launcher steps of the workspace spec hold: one open session for each
   job, a build before each session, a held process, and a start only if the
   session did not end.
5. Three adapters: `claude-code`, `codex`, and each generic command of the
   file `agents.toml`.
6. The session gets its own socket, `.iris/socket` in the workspace. The
   socket is the only capability of the session. It holds no token.
7. The engine builds the workspace of each done job again, so its mail files
   go and the files of the agent stay (workspace spec).
8. The engine starts an agent only in an isolation that keeps it away from
   the data folder, `iris.sock` and the network (see
   [Isolation](#isolation)). This card defines the isolation interface.
   R4-05 gives the isolation of macOS and of Linux. Until R4-05 merges,
   `iris job run` refuses each run.

## Not in scope

- The isolation of each system: no network, no data folder and no host
  socket (R4-05). This card gives the interface and refuses a run with no
  isolation, so no agent runs outside an isolation.
- The write verbs in a session and the one draft changeset of a session
  (R4-03).
- The read verbs `search`, `show` and `list` in a session. The session socket
  refuses them in this card, with the reason, and the agent reads the files
  of the workspace. A later card adds them with the grant rules of the
  workspace spec (known limit, with a GitHub issue).
- `iris wait`, `iris ask` and the wake (release 5). A wake that starts a
  session calls the same launcher.
- A start from the web client (R4-10). The web client calls the same
  launcher.
- The comments of a review that return to the agent (R4-08), and Stop for
  all sessions (R4-11).

## Design

### Sessions in the store

Migration 20 adds the table `job_session`:

| Column | Type | Rule |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | The session ID is `se-<id>`. |
| `job_id` | INTEGER NOT NULL | References `job(id)`. |
| `agent` | TEXT NOT NULL | The adapter name, 1 to 64 characters of `a-z`, `0-9` and `-`. |
| `registered_at` | INTEGER NOT NULL | Milliseconds. |
| `started_at` | INTEGER | NULL until step 4 of the launch. |
| `ended_at` | INTEGER | NULL while the session is open. |
| `end_reason` | TEXT | NULL while open. Else one of `exit`, `stop`, `revoke`, `job_state`, `time_limit`, `engine`, `launch_failed`. |
| `exit_code` | INTEGER | The exit code of the agent for `exit`, else NULL. |
| `reaped_at` | INTEGER | NULL until each process of the session stopped. |

A session has two ends. `ended_at` is the end of the grant: from that moment
the session socket answers nothing. `reaped_at` is the end of the
processes: the engine sets it after the helper exited and was reaped (see
[The hold helper](#the-hold-helper)).

An open session is a row with `ended_at IS NULL`. A live session is a row
with `reaped_at IS NULL`. Triggers hold these rules for each path that
writes SQL:

- A partial unique index on `job_id WHERE reaped_at IS NULL`: one live
  session for each job. A second `register` fails until the processes of
  the first session stopped. So a new build never runs while a process of
  the last session can still use the workspace, after a stop, a revoke, a
  time limit or an exit.
- An insert needs a job in the state `working`, and NULL in `started_at`,
  `ended_at`, `end_reason`, `exit_code` and `reaped_at`. An insert with the `id` of a row
  fails (the scope rule of R4-02: no replace).
- An update does one of three moves, and nothing else:
  - start: `started_at` from NULL to a value, only while `ended_at` is NULL;
  - end: `ended_at` and `end_reason` (and `exit_code` for `exit`) from NULL
    to values, once;
  - reap: `reaped_at` from NULL to a value, once, only after the end.
- A delete fails.
- After a revoke of a scope row of the job (`job_scope.revoked_at` set),
  each open session of the job ends with `revoke`, in the same transaction.
- After a move of the job to a state other than `working`, each open session
  of the job ends with `job_state`, in the same transaction.

So a revoke and a job move end the session in the store at once, also when
the engine is busy. The engine then stops the processes (see
[Supervisor](#supervisor)).

`iris_job` gives `register`, `start`, `end`, `reap`, `open_session` and
`live_sessions`, each with an `_in` form for one transaction.

### Adapters

An adapter gives the program, the arguments and the environment of the
agent. The prompt is fixed text. It holds no mail text and no goal text:

> Read JOB.md in this folder and do the job that it gives. Use only the
> files of this folder and the iris command.

| Adapter | Program and arguments |
|---|---|
| `claude-code` | `claude -p <prompt> --allowedTools "Bash(iris:*)" Read Write Edit Glob Grep` |
| `codex` | `codex exec --sandbox workspace-write <prompt>` |
| `<name>` from `agents.toml` | The `command` list of the file, then the prompt as the last argument. |

The file `<data dir>/agents.toml` is optional:

```toml
[agent.my-tool]
command = ["/usr/local/bin/my-tool", "--run"]
pass_env = ["MY_TOOL_KEY"]
```

- A name of the file can not be `claude-code` or `codex`.
- `command` has 1 to 32 strings. The first string is the program.
- `pass_env` names environment variables that the session gets, if the
  engine has them. Each name is `[A-Z_][A-Z0-9_]*`. A name that starts with
  `IRIS` is refused.
- The engine reads the file at each `run`. An error in the file refuses the
  run, with the line of the error. The file never changes a running session.

The environment of the agent is empty, then gets only these variables, if
the engine has them: `PATH`, `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_ALL`,
`TERM`, `TMPDIR`, `SHELL`, and the `pass_env` of the adapter. The built-in
adapters pass `ANTHROPIC_API_KEY` (`claude-code`) and `OPENAI_API_KEY` and
`CODEX_HOME` (`codex`): these are the keys of the agent tool, not of the
mailbox. No variable of Iris goes to the agent.

The working folder of the agent is the workspace. Its standard input is
`/dev/null`. Its standard output and standard error are pipes to the
helper. The helper copies both to `<data dir>/sessions/se-<id>.log` (mode
`0600`, `create_new`, opened by the engine and given to the helper as its
standard output). The helper writes at most 10 MiB to the log, writes one
line that says the rest is dropped, and then keeps reading the pipes and
drops the bytes, so the agent never blocks on a full pipe and the log never
grows past the limit.

### The hold helper

A held process must run no code of the agent until step 4 (workspace spec).
The engine starts the hidden command `iris session-hold` (the same program),
not the agent:

1. The engine starts `iris session-hold` with a pipe on its standard input,
   the log file on its standard output, the isolation and the adapter
   program and arguments as its arguments, and the environment of the agent.
2. The helper reads one line from the pipe. It runs no code of the agent
   before the line.
   - `start`: the helper starts the agent as its child, in the isolation,
     in a new process group of its own (`process_group(0)`). The helper is
     not in that group, so a signal to the group never reaches the helper.
   - Any other line, or the end of the pipe: the helper exits with no child.
3. After the start, the helper waits for two events: the child exits, or
   the pipe ends (the engine stopped, or the engine closed the pipe to stop
   the session).
4. On each of the two events, the helper stops the group of the agent:
   `SIGTERM` to the group, a wait of up to 5 seconds, then `SIGKILL` to the
   group. It does this also after a normal exit of the agent, so a
   background child that the agent left in its group stops too. Then the
   helper reaps the agent, and exits with the exit code of the agent (or
   the code of the signal).

So the pipe ties the session to the engine: when the engine process ends for
any reason, the kernel closes the pipe, and the helper stops the agent. The
helper waits for the group with `kill(-pgid, 0)`: the group has a process
while the call succeeds. A process that the agent moves out of its group
(`setsid`) is stopped by the isolation (R4-05), which ends each process of
the isolation with the session.

The helper is the same program as the engine, so no new file is installed.

### Launch

`iris_session::launch(store, data_dir, job, agent, now)` does the launcher
steps of the workspace spec:

1. Read `agents.toml` and resolve the adapter. A missing program fails here,
   before a row is written. Get the isolation of the system. If there is
   none, the run fails here with the reason: no isolation on this system.
2. Register: `iris_job::register` in a write transaction. It fails if the job
   is not `working`, or if the job has an open session.
3. Build: `iris_workspace::build`, then compare `Built::revoked` with
   `iris_job::revoked_count`. If they differ, build again, at most 5 times,
   then end the session with `launch_failed`.
4. Bind the session socket `.iris/socket` (mode `0600`) in the workspace.
   The full path of the socket must fit the socket address of the system
   (104 bytes on macOS). A longer path ends the session with `launch_failed`
   and the reason.
5. Start the hold helper (held).
6. Start: `iris_job::start` in a write transaction, only if the session did
   not end. If it ended (a revoke or a job move came during steps 3 to 5),
   close the pipe, wait for the helper, and go back to step 2 once. A second
   end of the same kind ends the launch with the reason.
7. Write `start` to the pipe.

Each failure after step 2 ends the session with `launch_failed`, closes the
pipe, waits for the helper if one started, sets `reaped_at` and removes the
socket, so no live session stays without a process. In step 6, a session
that ended is reaped the same way before step 2 runs again.

### Supervisor

The engine keeps one supervisor thread for all sessions. Each second, and
after each request on the engine socket that can end a session, it checks
each running session:

- The helper exited: reap it, end the session with `exit` and its exit
  code if it is still open, then set `reaped_at`. Then remove the socket
  file. If the job is `done`, build its workspace again.
- The session ended in the store (a revoke, a job move, a stop): close the
  pipe of the helper.
- The time limit passed: end the session with `time_limit`, then close the
  pipe. The default time limit is 30 minutes. `agents.toml` can set
  `[session] max_minutes`, from 1 to 1440.

### Stop

`iris job stop <j-id>` ends the open session of the job with `stop`. The
supervisor closes the pipe, and the helper stops the process group: `SIGTERM`,
then `SIGKILL` 5 seconds later. The stop time is the moment of the `SIGKILL`:
no process of the group runs after it. The isolation (R4-05) stops a process
that left the group.

After the helper exits, the engine reaps it, sets `reaped_at` and removes
the socket file. The workspace stays for the next session.

### Engine start and stop

- At the start of `iris serve`, before the engine socket opens, each live
  session belongs to the last engine. Its helper saw the end of the pipe
  when that engine stopped, and stops the group within 5 seconds. The new
  engine ends each open session with `engine`, waits 6 seconds after its
  own start, then sets `reaped_at` of each of these sessions. Until then a
  `run` of such a job fails at the register step.
- Then the engine builds the workspace of each done job whose workspace
  folder exists (workspace spec, "The granted messages of a job"): the mail
  files go, and `JOB.md`, `job.json`, `drafts/` and `notes.md` stay. It
  removes `sessions/` logs older than 30 days.
- After the reap of a session of a done job, the engine builds the
  workspace of the job the same way.

### Isolation

An isolation runs the agent so that it cannot reach the data folder, the
engine socket `iris.sock`, the socket of another session, or the network.
It shows the agent only the workspace (read and write), the programs of the
system and the folders that the adapter needs (read only, for example the
settings of the agent tool). It ends each process in it when the session
ends.

`iris_session::Isolation` is the interface: it gives the program and the
arguments that run the adapter command inside the isolation, and a check
that the isolation works on this system. This card gives no isolation for
a real system: R4-05 gives one for macOS and one for Linux. With no
isolation, `iris job run` fails with the reason, so no agent runs as the
OS user with access to the data folder (spec 2, C2 and C6).

The tests of this card use a test isolation that only the tests can make
(`#[cfg(test)]` or a test feature that the program does not enable). It
runs the command with no isolation, for the test scripts only.

### The session socket

The engine listens on `.iris/socket` of each running session. A request has
the same form as on `iris.sock` (one line of JSON). The engine knows the
session from the socket that took the request, never from the request.

The session socket answers `status` (the job: ID, goal, state, due date;
the counts of the workspace). R4-03 adds the engine side of the other
session verbs: one entry point that takes the request and the author of the
session (kind `agent`, `name` the adapter, `session` `se-<id>`, `job`
`j-<id>`) and applies the grant checks. When that entry point is on
`develop`, the session socket gives it each verb other than `status`. Until
then each other verb gets the exit code 3 and the reason. A host verb (for
example `brief`, `ledger`, `job run`) gets exit code 3 in each case.

Before each answer, the engine checks that the session is open in the
store. A request on the socket of an ended session gets exit code 3.

### The command

- `iris job run <j-id> --agent <name>` and `iris job stop <j-id>` are host
  verbs. They send a request on `iris.sock`, so only the engine starts and
  stops sessions.
- Session mode: if the current folder or a folder above it has `.iris/`,
  the command is in session mode. In session mode the command never opens
  the store, also for the read verbs of host mode. It sends each verb to
  `.iris/socket`. If the socket is not there, the command fails with exit
  code 4.

## Options not chosen

- A run with no isolation, with a warning: the agent could then read the
  key, the web secret and `iris.sock` of the data folder, and approve its
  own changesets (spec 2, C2 and C6). A warning does not keep these.
- A second OS user for the agent: it needs setup with administrator rights
  on each system, and it does not stop the network.

- A suspended process (`SIGSTOP`) as the held state: the agent program is
  already loaded, and a stop signal from outside can resume it. The helper
  runs no agent code at all.
- `PR_SET_PDEATHSIG` to stop the agent with the engine: Linux only. The pipe
  works on macOS and Linux.
- A start from `iris job run` in the terminal of the person, with no engine:
  then two processes could launch for one job, and nothing stops the
  session on a revoke.
- The goal text in the prompt: the goal is text of the person, but the agent
  reads it in `JOB.md`. A fixed prompt keeps all text out of the argument
  list, which other users of the system can see.
- A process library (`nix`, `rustix`, `command-group`): `std` gives the
  process group (`process_group`), the pipe and the wait. Only the signal
  to a process group needs `libc::kill`. `libc` is already in `Cargo.lock`.
  The code adds it as a direct dependency.

## Failure list

Supported input: a job ID of the store, an adapter name of the built-in list
or of `agents.toml`, and an `agents.toml` of the form above. All other input
is refused before a row is written.

| Input or event | Result | Test |
|---|---|---|
| A second `run` of a job with an open session | Refused at step 2 | Store test: second register fails |
| A `run` of a job that is not `working` | Refused at step 2 | Store test |
| A revoke during steps 3 to 6 | Step 6 finds the end, the agent never runs | Launch test with a revoke hook |
| A revoke after step 6 | The trigger ends the session; the supervisor stops the group | Supervisor test |
| The job moves to `done` while a session runs | Ends with `job_state`; after the reap the engine builds the workspace with no mail | Supervisor test |
| The agent program is missing | Refused at step 1 | Adapter test |
| The agent never exits | Ends at the time limit | Supervisor test with a short limit |
| The agent ignores `SIGTERM` | `SIGKILL` 5 seconds later; the helper is not in the group, so it lives to send it | Helper test with a `trap '' TERM` script |
| The agent exits and leaves a background child in its group | The helper stops the group after the exit | Helper test with `sleep 100 &` |
| A `run` right after a stop, a revoke or an exit, before the reap | Refused at the register step until `reaped_at` is set | Store test and launch test |
| The agent writes without end to its output | The log stops at 10 MiB; the helper keeps reading, so the agent does not block | Helper test with `yes` |
| A system with no isolation | `run` refused at step 1 | Launch test |
| The engine is killed (`SIGKILL`) | The pipe ends, the helper stops the group; the next engine start ends the row with `engine` | Helper test: close the pipe |
| The helper gets a line other than `start`, or the end of the pipe before it | Exits with no child | Helper test |
| A socket path longer than the system limit | `launch_failed` with the reason | Launch test with a long data folder |
| An `agents.toml` with an unknown key, a bad name, or an `IRIS` variable | Refused with the line | Parser test (property test over names) |
| A raw SQL update or delete of `job_session` outside the two moves | Refused by the triggers | Store test |
| A request on the session socket for another verb | Exit code 3, nothing changes | Socket test |
| An agent process that leaves the process group | The isolation ends it (R4-05) | R4-05 test |

## Impact

- `crates/iris-store/src/schema_20.sql`: the table and the triggers.
- `crates/iris-job`: the session functions.
- New crate `crates/iris-session`: adapters, `agents.toml`, the hold helper
  and the launch. A direct dependency on `libc` (exact version of the lock).
- `crates/iris`: the supervisor thread, the requests `job.run` and
  `job.stop` on `iris.sock`, the session socket, the verbs `iris job run`,
  `iris job stop` and the hidden `iris session-hold`, and session mode.
- Spec 1 section 5.1: `sessions/` and `agents.toml` in the data folder.
- Spec 2 section 8.2: the verbs `iris job run` and `iris job stop`.

Memory: the supervisor holds one small record for each running session
(the child handle, the pipe, the session ID). The log copy uses a buffer of
64 KiB for each session. The helper is a second copy of the program, but it
only waits: its resident memory is small, and the build measures it
(open question 1).

## Assumptions

- `claude -p` runs one prompt and exits, and `--allowedTools` limits its
  tools. Evidence: pending. The owner checks the line on the Mac (issue). If
  the flags differ, only the adapter table changes.
- `codex exec --sandbox workspace-write` runs one prompt and exits, with no
  network. Evidence: pending, the same check. The same tolerance.
- The kernel closes the write end of the pipe when the engine process ends
  for any reason, also `SIGKILL`. Evidence: POSIX `exit` and `close` rules.
  The helper test checks it.
- `std::process::Command::process_group` exists on macOS and Linux.
  Evidence: Rust 1.64 `CommandExt::process_group`.

## External writes

None. The launcher starts local processes and writes local files. The agent
writes changes only through the verbs, and nothing leaves before `submit`
(spec 2).

## Open questions

1. The resident memory of `iris session-hold`. The build measures it and
   writes it in the PR.

## Acceptance criteria

1. `iris job run` with the generic adapter of a test script starts the
   script in the workspace, and the script sees `JOB.md`. Red if the script
   does not run, or runs in another folder.
2. A second `run` of the same job fails while the first session is open.
   Red if two sessions of one job are open.
3. `iris job stop` ends the session, and no process of the group runs 6
   seconds later, also when the script ignores `SIGTERM`. Red if a process
   of the group runs.
4. A revoke during the build makes the agent never start. Red if the script
   writes its marker file.
5. A revoke while the agent runs ends the session with `revoke`, and stops
   the group. Red if a process of the group runs 6 seconds later.
6. After a kill of the engine, the agent stops, and the next engine start
   ends the session with `engine`. Red if the agent runs or the row stays
   open.
7. The environment of the agent has no variable that starts with `IRIS`, and
   no variable outside the list. Red if the script finds one.
8. In session mode, `iris search` fails with exit code 3 and does not open
   the store. Red if it prints a result.
9. With no isolation, `iris job run` fails before a session row is written.
   Red if a row or a process appears.
10. After `iris job stop`, a second `run` fails until the processes of the
    first session stopped, then succeeds. Red if a build starts while a
    process of the first session runs.
11. An agent that exits and leaves `sleep 100 &` has no running process 6
    seconds later. Red if the `sleep` runs.

## Verify

The tests run the generic adapter with test scripts in the test isolation.

```bash
cargo test -p iris-job
cargo test -p iris-session
cargo test -p iris
```

## Rollout

Migration 20 adds one table. The verbs are new. Nothing changes for a person
who does not run `iris job run`.

## Risks

- `iris job run` does nothing for the person until R4-05 merges. This is
  on purpose: no agent runs without an isolation.
- A wrong adapter line makes the agent fail at once. The log shows the error.
- Rollback: revert the code PRs. The table `job_session` stays and does no
  harm.

## Plan

1. R4-04 PR 1: this spec.
2. R4-04 PR 2: migration 20 and the session functions in `iris-job`.
3. R4-04 PR 3: the crate `iris-session`: adapters, `agents.toml`, the hold
   helper and the launch.
4. R4-04 PR 4: the engine: supervisor, requests, session socket, verbs and
   session mode.

## Spec checks

- [x] Each assumption holds with a second caller: the unique index makes a
  second launch fail.
- [x] Each assumption holds when an outcome is unknown: an engine crash
  closes the pipe; the next start ends the row.
- [x] Each dropped mechanism was dropped for a reason (Options not chosen).
- [x] No "idempotent" step is check-then-act: register and start are one
  write transaction each, with the triggers.
- [x] Each property has a test (Failure list).
- [x] The spec addresses the hardest problem: no agent code before the
  grant check of step 6, and no process after a stop.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported input and the safe result.
