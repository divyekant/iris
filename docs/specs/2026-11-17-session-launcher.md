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
   the data folder, `iris.sock` and the network, other than the model hosts
   of the adapter through the egress proxy of the engine (see
   [Isolation](#isolation) and [Model egress](#model-egress)). This card defines the isolation interface.
   R4-05 gives the isolation of Linux (sandbox spec,
   `docs/specs/2026-11-17-session-sandbox.md`). R4-05m gives the
   isolation of macOS. Until the card of a system merges, `iris job run`
   refuses each run on that system.

## Not in scope

- The isolation of each system: no network other than the egress proxy, no
  data folder and no host socket (R4-05 for Linux, R4-05m for macOS). This card gives the interface and refuses a run with no
  isolation, so no agent runs outside an isolation.
- The write verbs in a session and the one draft changeset of a session
  (R4-03).
- `iris wait`, `iris ask` and the wake (release 5). A wake that starts a
  session calls the same launcher.
- A start from the web client (R4-10). The web client calls the same
  launcher.
- The comments of a review that return to the agent (R4-08), and Stop for
  all sessions (R4-11).

## Design

### Sessions in the store

Migration 21 (20 went to R5-10) adds the table `job_session`:

| Column | Type | Rule |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | The session ID is `se-<id>`. |
| `job_id` | INTEGER NOT NULL | References `job(id)`. |
| `agent` | TEXT NOT NULL | The adapter name, 1 to 64 characters of `a-z`, `0-9` and `-`. |
| `registered_at` | INTEGER NOT NULL | Milliseconds. |
| `started_at` | INTEGER | NULL until step 4 of the launch. |
| `ended_at` | INTEGER | NULL while the session is open. |
| `end_reason` | TEXT | NULL while open. Else one of `exit`, `stop`, `revoke`, `job_state`, `time_limit`, `engine`, `launch_failed`, `helper_failed`. |
| `exit_code` | INTEGER | The exit code of the agent for `exit`, else NULL. |
| `helper_pid` | INTEGER | NULL until step 5 of the launch. The process ID of the hold helper, 2 or more (0 and 1 are never a session process). |
| `agent_pgid` | INTEGER | NULL until the helper starts the agent. The process group ID of the agent, 2 or more. |
| `reaped_at` | INTEGER | NULL until the engine confirmed that each process of the session stopped. |

A session has two ends. `ended_at` is the end of the grant: from that moment
the session socket answers nothing. `reaped_at` is the end of the
processes: the engine sets it only after the reap check passes (see
[The reap check](#the-reap-check)). A fixed wait never sets it.

An open session is a row with `ended_at IS NULL`. A live session is a row
with `reaped_at IS NULL`. Triggers hold these rules for each path that
writes SQL:

- A partial unique index on `job_id WHERE reaped_at IS NULL`: one live
  session for each job. A second `register` fails until the processes of
  the first session stopped. So a new build never runs while a process of
  the last session can still use the workspace, after a stop, a revoke, a
  time limit or an exit.
- An insert needs a job in the state `working`, and NULL in `helper_pid`,
  `agent_pgid`, `started_at`,
  `ended_at`, `end_reason`, `exit_code` and `reaped_at`. An insert with the `id` of a row
  fails (the scope rule of R4-02: no replace).
- An update does one of five moves, and nothing else:
  - helper: `helper_pid` from NULL to a value, once, only while `reaped_at`
    is NULL;
  - spawn: `agent_pgid` from NULL to a value, once, only while `reaped_at`
    is NULL and `helper_pid` is set;
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

`iris_job` gives `register`, `set_helper`, `set_agent`, `start`, `end`,
`reap`, `open_session` and `live_sessions`, each with an `_in` form for one transaction.

### Adapters

An adapter gives the program, the arguments and the environment of the
agent. The prompt is fixed text. It holds no mail text and no goal text:

> Read JOB.md in this folder and do the job that it gives. Use only the
> files of this folder and the iris command.

| Adapter | Program and arguments |
|---|---|
| `claude-code` | `claude -p <prompt> --allowedTools "Bash(iris:*)" Read Write Edit Glob Grep` |
| `codex` | `codex exec --sandbox workspace-write --skip-git-repo-check <prompt>` (the workspace is not a Git repository; `codex exec --help` lists the flag) |
| `<name>` from `agents.toml` | The `command` list of the file, then the prompt as the last argument. |

The engine finds the program of an adapter by its absolute path, or in the
absolute folders of its own `PATH`. It skips a relative folder or an empty
entry of `PATH`, because the working folder of the engine is not fixed (a
service). The program must be an executable file.

The file `<data dir>/agents.toml` is optional:

```toml
[agent.my-tool]
command = ["/usr/local/bin/my-tool", "--run"]
pass_env = ["MY_TOOL_KEY"]
allow_hosts = ["api.my-tool.example"]
```

- A name of the file can not be `claude-code` or `codex`.
- `command` has 1 to 32 strings. The first string is the program.
- `pass_env` names environment variables that the session gets, if the
  engine has them, at most 32. Each name is `[A-Z_][A-Z0-9_]*`. A name that starts with
  `IRIS` is refused. The proxy names (`HTTPS_PROXY`, `HTTP_PROXY`,
  `ALL_PROXY`, `NO_PROXY`, in any case) are refused, because only the
  engine sets them.
- `allow_hosts` names the HTTPS hosts that the agent can reach through the
  egress proxy, at most 16. The person sets them, and Iris does not check
  that a host is a model host: the agent tool can send the workspace to
  each of them. The built-in adapters have only their model host. Each one is a DNS name in lower case, with no port and
  no wildcard. An IP address is refused. The default is no host: the agent
  has no network.
- The engine reads the file at each `run`. An error in the file refuses the
  run, with the line of the error. A file larger than 64 KiB is refused. The file never changes a running session.

The environment of the agent is empty, then gets only these variables, if
the engine has them: `PATH`, `HOME`, `USER`, `LOGNAME`, `LANG`, `LC_ALL`,
`TERM`, `TMPDIR`, `SHELL`, and the `pass_env` of the adapter. The built-in
adapters pass `ANTHROPIC_API_KEY` and `CLAUDE_CODE_OAUTH_TOKEN` (a token
of `claude setup-token`, for a Claude subscription) (`claude-code`), and
`OPENAI_API_KEY` and `CODEX_HOME` (`codex`): these are the keys of the agent tool, not of the
mailbox. The Linux sandbox shows no host folder other than the workspace
and the program files: `HOME` is an empty private folder, and a
`CODEX_HOME` outside the view is not visible. So the agent tool reads no
settings or login files of the person, and authenticates only with a key
variable (sandbox spec, "Files"; known limit with an issue). A session with an egress port also gets `HTTPS_PROXY` and
`https_proxy`, set by the engine to the port of the session (see
[Model egress](#model-egress)); a value of the engine for these names is
never passed. No variable of Iris goes to the agent.

The working folder of the agent is the workspace. Its standard input is
`/dev/null`. Its standard output and standard error go to one pipe to the
helper. The helper reads the pipe and drops the bytes: no output of the
agent is stored. The agent can read a credential (a passed key, or a file
of the agent tool), and a filter cannot find each form of it, so the only
safe log of agent output is none. The helper reads without end, so the
agent never blocks on a full pipe.

The session log `<data dir>/sessions/se-<id>.log` (mode `0600`,
`create_new`, opened by the engine and given to the helper as its
standard output) holds only status lines that the helper writes from
fixed text and numbers: the start, the process group ID, each signal that
the helper sends, the exit code or signal of the agent, the count of
output bytes that the helper dropped, and the result of `Isolation::stop`.
After the reap, the helper waits at most 2 seconds for the end of the
output pipe, so that the count holds each byte. If a process that the stop
missed keeps the pipe open, the line gives the count as a lower bound and
says that the pipe is still open.
A status line holds no text of the agent. The log has a limit of 64 KiB.
The agent writes its own notes in `notes.md` of the workspace.

### The hold helper

A held process must run no code of the agent until step 4 (workspace spec).
The engine starts the hidden command `iris session-hold` (the same program),
not the agent:

1. The engine starts `iris session-hold <data dir> <hold args>` with a
   pipe on its standard input, the log file on its standard output, and
   the environment of the agent. `<data dir>` is the absolute data folder
   that the engine resolved (also from `--data`), and `<hold args>` are
   the session ID, the isolation and the adapter program and arguments.
   The helper calls `system(<data dir>)` with it. The data folder is an
   argument of the helper only: the helper does not pass it to the agent,
   whose arguments are the adapter arguments of `command()` and whose
   environment has no variable of Iris.
2. The helper reads one line from the pipe. It runs no code of the agent
   before the line.
   - `start`: the helper starts the agent as its child, in the isolation,
     in a new process group of its own (`process_group(0)`). The helper is
     not in that group, so a signal to the group never reaches the helper.
     Then the helper writes one line `started <pgid>` on its standard
     error, which is a second pipe to the engine. The engine writes
     `agent_pgid` (the spawn move).
   - Any other line, a line longer than 64 bytes, or the end of the pipe:
     the helper exits with no child.
   - If the start of the agent fails, the helper writes one line `failed`
     on its second pipe and exits with no child.

   The helper runs outside the sandbox of the agent: it is code of Iris,
   and it needs `/proc`, the data folder and the host loopback address.
   The agent is its child, inside the sandbox, and the isolation binds the
   life of each process of the session to the helper (see
   [Isolation](#isolation), lifetime rule). On Linux the child is `bwrap`,
   which ends with the helper (`docs/specs/2026-11-17-session-sandbox.md`).
3. After the start, the helper waits for two events: the child exits, or
   the pipe ends (the engine stopped, or the engine closed the pipe to stop
   the session).
   The helper uses one thread to read the output, and one thread for each
   event. If one of these threads does not start, the helper cannot read
   the output or see an event, so it does step 4 at once.
4. On each of the two events, the helper stops the group of the agent. It
   does not reap the agent before the last signal: it sees the exit with
   `waitid(P_PID, pid, WEXITED | WNOWAIT)`, which leaves the agent as a
   zombie. A zombie keeps its process ID, so the system cannot give the
   group ID to another process, and a signal to the group never reaches
   an unrelated process. The steps:
   1. `SIGTERM` to the group.
   2. A wait of 5 seconds. The helper cannot end the wait early from a
      check of the group, because the zombie keeps the group alive.
   3. `SIGKILL` to the group.
   4. `Isolation::stop` of the session, which ends each process that left
      the group.
   5. The reap of the agent (only now). Then the helper writes one status
      line to the engine on its second pipe, `exited <code>` or
      `signaled <signal>`, and exits.

   The helper does these steps also after a normal exit of the agent, so a
   background child that the agent left in its group stops too.

So the pipe ties the session to the engine: when the engine process ends for
any reason, the kernel closes the pipe, and the helper stops the agent. A
process that the agent moves out of its group
(`setsid`) is stopped by `Isolation::stop`, which the helper calls on the
end of the pipe with no engine. If the helper is killed too, the lifetime
rule of the isolation ends each process of the session. So no process of a
session outlives both the engine and the helper.

The helper is the same program as the engine, so no new file is installed.

### Launch

`iris_session::launch::Launcher` does the launcher steps of the workspace
spec. `begin` does steps 1 and 2, so the engine can answer `job.run` with
the session ID. `finish` does steps 3 to 6. The engine gives the store, the
data folder, the isolation, the command of the helper, the egress port, its
environment and its clock:

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
5. Start the hold helper (held), and write `helper_pid` (the helper move)
   in the same step. If the write fails, close the pipe and wait for the
   helper. The arguments of the helper are the session ID, then the
   program and the arguments that `Isolation::command` gives. The helper
   starts in the workspace folder, with the environment of the agent only.
6. Start and release, as one gate: open a write transaction that takes
   the write lock first (an `UPDATE` that changes no row takes it, as
   `BEGIN IMMEDIATE` does), and check that the session did not end. If it
   ended during steps 3 to 5, roll back, close the pipe, wait for the
   helper and run the reap check. Then act on the end reason:
   - `revoke`: the grant changed, so go back to step 2 once, with a new
     build. A second `revoke` ends the launch with the reason.
   - Each other reason (`stop`, `job_state`, `engine`): the launch ends
     with that reason, with no retry. So a stop during the launch cancels
     the run. `iris job run` already answered after step 2 (see
     [The command](#the-command)), so it does not report this end: `iris
     job session` shows the session as ended with `stop` and no
     `started_at`.

   The engine also keeps a stop mark for each job whose stop came during
   the launch. The gate reads the mark in the same transaction. If the
   mark is set, the gate rolls back and ends the session with `stop`, also
   when the end write of the stop failed. So no agent starts after a stop
   (issue #517). The mark has its own lock, and no code waits for the
   store while it holds that lock.

   If the session did not end, then in the same transaction:
   1. Set `started_at` (`iris_job::start_in`).
   2. Write `start` to the pipe.
   3. Wait for the line `started <pgid>` of the helper, for at most 2
      seconds. Write `agent_pgid` (`iris_job::set_agent_in`).
   4. Commit.

   If the line does not come in 2 seconds, or the commit fails, roll
   back, close the pipe, and end the session with `launch_failed`. The
   helper then stops the agent if it started one.

   SQLite lets one write transaction run at a time, and a revoke or a job
   move is a write transaction. So a revoke commits either before the gate
   (the gate sees the end, and the helper never gets `start`), or after
   the commit of the gate. At that commit the agent process exists and its
   group is in the store, so the revoke ends a running session, and the
   supervisor stops it. No revoke can come between the check and the
   spawn. The gate holds the write lock for at most 2 seconds, less than
   the `busy_timeout` of 5 seconds of the store, so a revoke waits and
   then succeeds.

Each failure after step 2 ends the session with `launch_failed`, closes the
pipe, waits for the helper if one started, runs the reap check and removes
the socket. The wait for the helper is at most 15 seconds (the helper needs
5 seconds and the isolation stop). After it, the launch kills the helper,
and the lifetime rule of the isolation ends each process of the session. So no live session stays without a process. In step 6, a session
that ended is reaped the same way before step 2 runs again.

### Supervisor

The engine keeps one supervisor thread for all sessions. Each second, and
after each request on the engine socket that can end a session, it checks
each running session:

- The helper exited: reap it. If the helper wrote `exited <code>` or
  `signaled <signal>`, the agent result is known: end the session with
  `exit` and that code (a signal gives NULL) if it is still open. If the
  helper exited with no such line (it was killed, or it failed), the
  agent result is unknown: end the session with `helper_failed` and NULL
  in `exit_code`, if it is still open, and call `Isolation::stop`. In both
  cases, run the reap check, which keeps the session live while a process
  of the agent group runs. Then remove the socket file. If the job is
  `done`, build its workspace again after the reap.
- An open session with no launch thread of this engine and no helper
  (`helper_pid` NULL, or the helper ended): end it with `launch_failed`, then
  run the reap check. This covers a launch that failed and could not write
  the end (for example, a full disk): the launch tries the end write 3
  times, then reports the session as open, never as ended.
- A session that ended but did not pass the reap check: run the reap check
  again. The session stays live until the check passes.
- The session ended in the store (a revoke, a job move, a stop, or a verb
  of the session that ended it): run the shutdown order below.
- The time limit passed: end the session with `time_limit`, then run the
  shutdown order. The default time limit is 30 minutes. `agents.toml` can set
  `[session] max_minutes`, from 1 to 1440.

The shutdown order, the same for each end of a session (also the exit of
the agent, which the helper reports, and a launch that fails after its
proxy started):

1. End the grant: the end is in the store. From now on the session socket
   takes no new request (exit code 3).
2. Close the egress proxy of the session (`Proxy::close`, see "Model
   egress"). This comes before each wait: no step below keeps a tunnel
   open. The proxy keeps its port bound. The engine stops the proxy
   (`Proxy::stop`, which frees the port) only after the reap check of the
   session passes. A launch that fails before the helper starts closes
   and keeps its proxy in the same way, and the supervisor stops it after
   the reap check.
3. Finish the accepted requests: each request that passed the check of the
   session socket before the end gets its reply, for at most 10 seconds.
   Then close the session socket and each of its connections.
4. Close the pipe of the helper. The helper stops the group and calls
   `Isolation::stop`.
5. Call `Isolation::stop` from the engine too, and run the reap check.

So a verb that ends its own session (`iris done`) gets its reply before
any process of the agent gets a signal.

### Stop

`iris job stop <j-id>` ends the open session of the job with `stop`. The
supervisor closes the pipe, and the helper stops the process group: `SIGTERM`,
then `SIGKILL` 5 seconds later, then `Isolation::stop`, which ends each
process that left the group. The grant ends at the end in the store: from
that moment the session socket answers nothing.

The stop is complete when the reap check passes (`reaped_at` set): only
then does no process of the session run. `iris job stop` waits up to 15
seconds for `reaped_at`. If it is set, the command prints that the session
stopped and exits with 0. Else it prints that the session ended but its
processes still stop, with the reason of the reap check, and exits with 1.
The supervisor keeps the stop going in each case.

After the helper exits, the engine reaps it, runs the reap check and
removes the socket file. The workspace stays for the next session.

### Engine start and stop

- At the start of `iris serve`, before the engine socket opens, each live
  session belongs to the last engine. Its helper saw the end of the pipe
  when that engine stopped, and stops the group within 5 seconds. The new
  engine ends each open session with `engine`. The supervisor then runs
  the reap check of each of these sessions on each turn. Until the check
  passes, a `run` of such a job fails at the register step.

### The reap check

The engine sets `reaped_at` only when it confirms that each process and each launch step of the
session ended. The check passes only if all of these are true:

0. No launch thread of this engine holds the session. The launch thread
   (steps 3 to 7) owns the session from the register until it returns: it
   does each build, the bind and the removal of `.iris/socket`, and the
   start of the helper. The engine keeps the set of the session IDs of the
   running launch threads. The supervisor does not run the reap check of a
   session in the set. The launch thread checks the end of the session
   after each step and returns at the first check after an end, after its
   own cleanup. A build is not stopped in its middle, so the wait is at
   most the time of one build. After an engine restart, no launch thread
   exists, so rule 0 holds.

1. The helper ended. This engine reaped the helper (`wait` gave its exit),
   or `helper_pid` is NULL, or `kill(helper_pid, 0)` fails with `ESRCH`.
2. The group of the agent is empty. `agent_pgid` is NULL, or
   `kill(-agent_pgid, 0)` fails with `ESRCH`.
3. `Isolation::stop` of the session returned with no error, and the
   isolation confirms that it holds no process of the session
   (`Isolation::ended`). The test isolation has no process of its own, so
   it confirms at once.

If one of these is false, or a call gives an error other than `ESRCH`,
the check fails, `reaped_at` stays NULL, and the session stays live. The
supervisor runs the check again on its next turn. A session that is still
live 10 minutes after its end gets one line in the engine log with the
reason. It never gets `reaped_at` from a wait or a time.

Rule 1 also covers a helper that was killed while the agent lives: rule 2
then keeps the session live until the group is empty. A process ID that
the system gives to a new, unrelated process keeps the session live (a
false "still runs"), never a false "ended". This is a known limit with an
issue: the person then waits for the process to end, or restarts the
machine.
- Then the engine builds the workspace of each done job whose workspace
  folder exists and that has no live session (workspace spec, "The granted
  messages of a job"): the mail files go, and `JOB.md`, `job.json`,
  `drafts/` and `notes.md` stay. A done job with a live session gets this
  build only after the reap check of the session passes (next point). It
  removes `sessions/` logs older than 30 days.
- After the reap of a session of a done job, the engine builds the
  workspace of the job the same way.
- One build thread of the engine does these builds, in order. A build can
  wait for the workspace lock with no limit, so no supervisor turn, stop
  or run waits for a build.
- In the same way, one stop thread of the engine stops the egress proxy
  of each reaped session (`Proxy::stop`). A proxy stop joins the threads
  of the proxy, so no supervisor turn, stop or run waits for it.
- A supervisor turn does only fast work: the end in the store, the
  `Proxy::close`, and the close of the socket and the pipe. Each ended session gets its own reap
  thread for `Isolation::stop` and the reap check, which have no time
  limit. A turn starts no second reap thread for a session while one
  runs. So a slow session never holds the stop of another session.

### Isolation

An isolation runs the agent so that it cannot reach the data folder, the
engine socket `iris.sock`, the socket of another session, or the network.
The one exception is the egress port of the session (see
[Model egress](#model-egress)).
It shows the agent only the workspace (read and write), a private empty
home and `/tmp` (read and write), and the programs and system files of the
sandbox spec (read only). It shows no settings or credential file of the
person. It ends each process in it when the session ends.

`iris_session::Isolation` is the interface. It gives:

- the program and the arguments that run the adapter command inside the
  isolation, with the egress port as the only network that it permits;
- a check that the isolation works on this system;
- `name(session)`: the durable identity of the isolation of one session,
  for example a cgroup path on Linux. It depends only on the session ID,
  so a new engine finds the isolation of a session after a restart.
- `stop(session)`: ends each process in the isolation of the session, also
  a process that left the process group (`setsid`). It returns when no
  process of the session runs, or with an error.
- `ended(session)`: true only if the isolation holds no process of the
  session (rule 3 of the reap check).
- The lifetime rule: when the helper process ends for any reason, also
  `SIGKILL`, each process in the isolation of the session ends, with no
  call from the engine. For example on Linux, the agent runs in a new PID
  namespace whose first process is a child of the helper with
  `PR_SET_PDEATHSIG`; when the first process of a PID namespace ends, the
  kernel kills each process in it. R4-05 (Linux) and R4-05m (macOS) give the mechanism of each
  system, and an isolation that cannot keep this rule does not pass the
  check of the system.

The engine calls `stop` on each end of a session, in step 5 of the
shutdown order (see [Supervisor](#supervisor)): on a stop, a revoke, a job
move, a time limit, an exit of the agent, a failed helper, a failed launch,
and at the engine start for each live session. The
supervisor calls `stop` again on each turn while the reap check fails. So
no process of a session runs past its end, also when the helper is gone.

This card gives no isolation for a real system: R4-05 gives one for Linux, and R4-05m one for macOS. With no
isolation, `iris job run` fails with the reason, so no agent runs as the
OS user with access to the data folder (spec 2, C2 and C6).

The tests of this card use a test isolation that only the tests can make
(`#[cfg(test)]` or a test feature that the program does not enable). It
runs the command with no isolation, for the test scripts only.

### Model egress

The built-in agent tools call their model over HTTPS: `claude-code` needs
`api.anthropic.com`, and `codex` needs `api.openai.com`. An isolation with
no network stops them. The engine gives each session one narrow path out:

- The engine listens on one TCP port of `127.0.0.1` for each running
  session, the egress port. The isolation permits a connection to this
  port only. It blocks each other address, also DNS.
- The proxy takes a client only from the session address of its session
  (`127.128.0.0` plus the session ID). The relay of the sandbox connects
  from that address (sandbox spec, "Egress on Linux"). A client from each
  other address gets `403` and the log word `not_session`.
- The agent gets `HTTPS_PROXY=http://127.0.0.1:<port>` (and `https_proxy`).
- The proxy takes only `CONNECT <host>:443`. The host must be in the
  allowed hosts of the adapter: `api.anthropic.com` for `claude-code`,
  `api.openai.com` for `codex`, and `allow_hosts` for an adapter of
  `agents.toml`. The engine resolves the host itself, and checks each
  address that it connects to: it refuses a loopback, private, link-local,
  unique local, multicast, unspecified, documentation or other reserved
  address (IPv4 and IPv6, also an IPv4 address mapped in IPv6). The proxy
  connects only to the checked address, on port 443 whatever port the
  resolver gives, never to the name again, so a
  second resolution cannot change it. If no address passes, the request
  gets `403` with the reason `address_not_public`. A local model host is
  not supported in a session (known limit, with an issue). Each other request,
  host or port gets `403` and closes.
- Two names can share one address (a CDN), so the name of the `CONNECT`
  alone does not keep other sites out. After the `200`, the proxy reads
  the first TLS message of the agent, the ClientHello (at most 16 KiB, in
  10 s), with the parser of `rustls`. The tunnel opens only if its server
  name (SNI) is the host of the `CONNECT`. Else the proxy closes the
  connection with the reason `sni_not_allowed`, or `not_tls` for bytes
  that are not a ClientHello. The proxy sends the ClientHello on to the
  host, then relays the bytes and does not read them. TLS runs from the
  agent to the model host, so the engine sees no key and no text. A site
  of the same CDN that serves a request with another name inside TLS
  (domain fronting), or a client that hides the name with Encrypted
  Client Hello, is a known limit (issue).
- The proxy writes one line for each connection to its own file,
  `<data dir>/sessions/se-<id>.egress.log` (mode `0600`). The launch
  starts one proxy for each session that it registers, after the bind and
  before the helper, with the log and the peer address of that session.
  A revoke retry registers a second session, so it gets its own proxy. If
  the proxy does not start, the session ends with `launch_failed`. For an allowed
  `CONNECT`, the line has the host (a value of the allowed list, as the
  list writes it), the bytes in each direction, and the result
  (`closed`, `stopped` or `connect_failed`). For a refused request, the
  line has only a fixed reason (`not_connect`, `host_not_allowed`,
  `port_not_allowed`, `address_not_public`, `too_long`, `too_many` for a
  ninth open connection or a resolver worker past the bound, `timeout`
  for a head or a ClientHello that does not come in 10 s, `not_tls`,
  `sni_not_allowed`, `no_resources` when the engine has no file
  descriptor or thread for the connection, `not_session` for a client
  that is not at the session address) and no text of the request. For a connection that closes, or
  that a stop ends, before a full head, the line has `ended` and
  `no_head` or `stopped`. So each connection gets exactly one line.
  Only the proxy writes this file,
  so it needs no shared append path with the helper. The file has a limit
  of 1 MiB. At the limit the proxy writes one line that says the rest is
  dropped, then only counts: at the end of the session it writes one last
  line with the count of dropped lines, inside a reserved 4 KiB. So the
  two logs of a session never pass 64 KiB and 1 MiB. An egress line holds
  no text of the agent: only a value of the allowed list, numbers and
  fixed words.
- The proxy resolves and connects in a worker thread, for at most 10 s.
  A stop does not wait for the worker: a stalled resolver cannot hold the
  end of a session. A worker that is still in the resolver stays until the
  resolver returns, then drops its connection. It holds only the host name
  and the resolver, not the log or the streams of the proxy. At most 16
  workers live in the Iris process at one time, over all sessions, also
  after a stop; a `CONNECT` past this bound gets `403` with `too_many`.
- When the session ends (`ended_at` is set: a stop, a revoke, the exit of
  the agent or a time limit), the supervisor closes the proxy on its next
  turn (`Proxy::close`, step 2 of the shutdown order, before each wait). The supervisor turns each second, and a
  `iris job stop` or a revoke through the engine also wakes it. The close
  ends each open tunnel and each resolve in progress, and from then on
  the proxy answers each new connection with a close and the log line
  `ended reason=stopped`. The close keeps the port bound, and the engine
  stops the proxy (`Proxy::stop`, which frees the port) only when the
  reap check of the session passes.
- Known limit (issue): an end that a store trigger or another process
  writes is not in step with the proxy. Between the commit of `ended_at`
  and the next supervisor turn (at most about 1 second), an open tunnel
  can still send bytes to the model host. The person chose that host for
  the job, and it had the scope of the session during the whole grant.
  The end of a grant stops the verbs of the session at once (each verb
  reads `ended_at` in its transaction). It does not call back what the
  agent sent before. A gate in each tunnel that reads the store for each
  byte costs more than this window is worth.
  The relay of the Linux sandbox (`docs/specs/2026-11-17-session-sandbox.md`)
  connects to the port while the helper lives, and the reap check
  confirms that the helper ended. So the port number cannot go to the
  proxy of another session while a process of this session can reach it.

The engine knows the session from the port, never from the request. An
adapter with no allowed host gets no egress port and no `HTTPS_PROXY`.

The agent sends the files of the workspace, which hold granted mail, to
its model host. The person chose the adapter and its model host, as with
the model of Iris (spec 1). The proxy keeps all other hosts out, so the
agent cannot send mail to another place.

### The session socket

The engine listens on `.iris/socket` of each running session. A request has
the same form as on `iris.sock` (one line of JSON). The engine knows the
session from the socket that took the request, never from the request.

The session socket answers these verbs of this card:

- `status`: the job (ID, goal, state, due date) and the counts of the
  workspace.
- `search`, `show` and `list`: the read verbs with the grant rules of the
  workspace spec ("The same grant limits each verb of a session"). A
  search runs only over granted messages: a message of
  `job_scope_message` of an active ref of the job that is also clean mail
  now (the predicate of the workspace build: `Mail::for_reader`, not
  hostile, not gone, not a signal source). It adds nothing to the
  workspace. `list` gives only the items of the workspace that the current
  grant still holds: each source of the item must pass the same check as
  search now, also when the item file is still in the folder. `show <i-id>`
  gives such an item with the same fields. `show <m-id>` gives a granted message (the same check as
  search) with the fields of a message of a thread file of the workspace:
  ID, date, the clean From, To, Cc and Subject, and the clean text. Any
  other ID, or a message that is not granted, gets exit code 3 with "not
  in this job", the same answer for an ID that does not exist.
  The engine reads the grant at each call, so a revoke takes effect at
  once. These verbs come in their own card (R4-04f, see the plan). Until it merges,
  the socket refuses them with exit code 3 and the reason.

R4-03 adds the engine side of the other
session verbs: one entry point that takes the request and the author of the
session (kind `agent`, `name` the adapter, `session` `se-<id>`, `job`
`j-<id>`) and applies the grant checks. When that entry point is on
`develop`, the session socket gives it each verb that is not in the list
above. Until
then each other verb gets the exit code 3 and the reason. A host verb (for
example `brief`, `ledger`, `job run`) gets exit code 3 in each case.

The engine checks that the session is open in the store when it takes a
request, before it runs the verb. A request that arrives after the end
gets exit code 3. A request that passed the check runs, and its reply goes
back on the same connection, also when the verb itself ends the session
(for example `iris done` of R4-03, which moves the job to `done` and so
ends the session in the same transaction). The engine closes the session
socket and each of its connections only after the reply of each request
that passed the check, or after 10 seconds. The supervisor closes the pipe
of the helper after that. A request on a connection after the end gets
exit code 3.

### The command

- `iris session-hold` is not a verb of the command line: the program
  checks this first argument before it reads the command line, and the
  verb list (and the MCP tools) does not show it. Its command line is
  `iris session-hold <data folder> <hold arguments>`. The helper builds the
  isolation with the same data folder as the engine, because the isolation
  keeps a record of each session there (R4-05).
- `iris job run <j-id> --agent <name>` and `iris job stop <j-id>` are host
  verbs. They send a request on `iris.sock`, so only the engine starts and
  stops sessions.
- The engine answers `job.run` right after step 2 of the launch (the
  register), with the session ID `se-<id>`, or with the refusal of steps 1
  and 2. Steps 3 to 7 then run on a thread of the engine, not inside the
  request. So the answer comes in well under the 30 seconds of the socket
  client, also for a slow build. `iris job run` prints the session ID and
  that the session is registered, and exits with 0. Exit code 0 means
  only that the register succeeded. Each later result of the launch (a
  start, a `launch_failed`, a stop before the start) is in the row, and
  `iris job session` shows it.
- `iris job session <j-id>` (host verb) shows the last session of the job:
  its ID, the agent, and its state: `starting` (open, no start),
  `running` (open, started), `ending (<reason>)` (ended, not reaped yet)
  or `ended (<reason>)` (ended and reaped). It reads the row of the store,
  so it works after a time-out or a lost connection of `iris job run`.
- If the answer of `job.run` is lost (a time-out, a broken connection),
  `iris job run` prints that the result is unknown and tells the person to
  run `iris job session <j-id>`. A second `run` of the job is safe: it is
  refused at step 2 while the first session is live, with the ID of that
  session, so it never starts a second agent.
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
- Full network for the built-in adapters: the agent could then send mail
  text to any host. The proxy permits only the model host.
- A proxy that opens TLS to read requests: it needs a local certificate
  authority that the agent tool trusts, and it adds no rule that the host
  list does not give.
- A log of agent output with a filter of secret values: a filter finds only
  the exact values that Iris passed, not a key that the agent reads from a
  file or prints in another form.
- A wait of a fixed time before `reaped_at`: a process that ignores the
  signals, or a stuck kill, would let a new build run beside it.

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
| A revoke during steps 3 to 5 | Step 6 finds the end, the helper never gets `start` | Launch test with a revoke hook |
| A revoke at the same moment as step 6 | Waits for the write lock of the gate; it commits before the check (no start) or after the commit (a running session, ended at once) | Launch test: a revoke from a second connection while the gate holds the lock |
| A raw insert with `helper_pid` or `agent_pgid` set | Refused by the trigger | Store test |
| A revoke after step 6 | The trigger ends the session; the supervisor stops the group | Supervisor test |
| The job moves to `done` while a session runs | Ends with `job_state`; after the reap the engine builds the workspace with no mail | Supervisor test |
| The agent program is missing | Refused at step 1 | Adapter test |
| The agent never exits | Ends at the time limit | Supervisor test with a short limit |
| The agent ignores `SIGTERM` | `SIGKILL` 5 seconds later; the helper is not in the group, so it lives to send it | Helper test with a `trap '' TERM` script |
| The agent exits and leaves a background child in its group | The helper stops the group after the exit | Helper test with `sleep 100 &` |
| A `run` right after a stop, a revoke or an exit, before the reap | Refused at the register step until `reaped_at` is set | Store test and launch test |
| The agent writes without end to its output | The helper drops the bytes and counts them; the agent does not block | Helper test with `yes` |
| A system with no isolation | `run` refused at step 1 | Launch test |
| The engine is killed (`SIGKILL`) | The pipe ends, the helper stops the group; the next engine start ends the row with `engine` | Helper test: close the pipe |
| The helper gets a line other than `start`, or the end of the pipe before it | Exits with no child | Helper test |
| A socket path longer than the system limit | `launch_failed` with the reason | Launch test with a long data folder |
| An `agents.toml` with an unknown key, a bad name, or an `IRIS` variable | Refused with the line | Parser test (property test over names) |
| A raw SQL update or delete of `job_session` outside the five moves | Refused by the triggers | Store test |
| A request on the session socket for another verb | Exit code 3, nothing changes | Socket test |
| An agent process that leaves the process group | `Isolation::stop` ends it on each end of the session; the reap check needs `ended` | Test isolation that records the calls; R4-05 test on each system |
| A done job with a live session at the engine start | The build waits for the reap check of the session | Supervisor test |
| A `search` in a session for a word of a message outside the grant | No result | Socket test |
| A `search` or `show` in a session of a listed message that is now hostile, gone or a signal source | No result; `show` gives "not in this job" | Socket test |
| A `list` or `show` of an item whose source became hostile after the build | Not listed; `show` gives "not in this job" | Socket test |
| A verb that ends its own session (`iris done`) | Its reply comes back; the next request gets exit code 3 | Socket test with a test verb that ends the session |
| `iris job stop` while a process that left the group still runs | Exit code 1 with the reason; the supervisor keeps calling `Isolation::stop` | Supervisor test with a test isolation whose `stop` fails once |
| The helper is killed and the agent group lives | The session ends with `helper_failed` and no exit code; the engine calls `Isolation::stop`; the reap check fails on rule 2 until the group is empty, and a `run` is refused | Supervisor test: kill the helper, keep the group |
| The engine restarts while the group still runs | The session stays live until the group is empty; no time sets `reaped_at` | Supervisor test with a group that ignores `SIGTERM` for 8 seconds |
| `kill` gives an error other than `ESRCH` | The check fails; the session stays live | Unit test of the check with a fake `kill` |
| A `CONNECT` to a host that is not allowed, or a port other than 443, or a request that is not `CONNECT` | `403`, closed, one egress log line with a fixed reason and no request text | Proxy test: `CONNECT <marker>:443` leaves no marker in the log |
| A `CONNECT` to an allowed host, then a ClientHello with another server name (a site that shares the address) | Closed before any byte reaches the host; `sni_not_allowed` in the egress log | Proxy test with two names on one test server |
| A revoke right after the gate commits, before the agent reads a file | The agent exists at the commit (the gate waited for `started`), so the revoke ends a running session and the supervisor stops it | Launch test: revoke hook right after the commit; the session ends with `revoke` and its group stops |
| The helper does not confirm the spawn in 2 seconds | Roll back; `launch_failed`; the helper stops a late agent | Launch test with a helper that waits 3 seconds |
| The agent and its children exit before the helper signals the group | The agent stays a zombie until after the last signal, so its group ID is not reused | Helper test: the agent exits at once, and the helper sends the signals before the reap (test hook that records the order) |
| A build that takes longer than the socket time-out | `job.run` already answered after the register; the launch goes on; `iris job session` shows it | Launch test with a slow build hook |
| The answer of `job.run` is lost | The command says the result is unknown and names `iris job session`; a retry is refused with the live session ID | Command test with a socket that closes before the answer |
| A revoke or a stop during the build, then at once a second `run` | The first session stays live until its launch thread returns after its cleanup; the second `run` is refused at the register until then | Launch test: a build hook that waits, a stop, then a second `run` that fails until the first thread returned |
| `iris job stop` during steps 3 to 5 of a launch | The launch ends with `stop`, no retry; `iris job session` shows the end with no start | Launch test with a stop hook |
| `iris job stop` during a launch, and its end write fails | The gate reads the stop mark: no agent starts, and the launch ends the session with `stop` | Engine test: a trigger refuses the end write of the stop |
| An agent that opens connections without end | The egress log stops at 1 MiB, then counts; the engine memory stays bounded (8 open connections) | Proxy test with 200 refused connections and a small limit |
| The engine is killed and an agent process left the group (`setsid`) | The helper sees the end of the pipe, stops the group and calls `Isolation::stop`; if the helper is killed too, the lifetime rule ends the process | Helper test with the test isolation that records `stop`; R4-05 test of the lifetime rule on each system |
| An agent that prints a passed key or a file of the agent tool | Nothing of it is stored; the log has only status lines | Helper test: a script prints a marker value to both pipes, and no file of the data folder holds it |
| A connection to the egress port of another session | The other session is not reached: each port belongs to one session, and the isolation permits only its own port | R4-05 test |
| An `allow_hosts` entry with a port, a wildcard or an IP address | Refused with the line | Parser test |
| An allowed host that resolves to a loopback, private or other reserved address | `403` with `address_not_public`; no connection | Proxy property test over the reserved ranges, with a test resolver |
| A `pass_env` with a proxy name, in any case | Refused with the line | Parser test |
| An adapter with no allowed host while the engine has `HTTPS_PROXY` | The agent gets no proxy variable | Launch test |

## Impact

- `crates/iris-store/src/schema_21.sql`: the table and the triggers.
- `crates/iris-job`: the session functions.
- New crate `crates/iris-session`: adapters, `agents.toml`, the hold helper,
  the launch, the reap check and the egress proxy. A direct dependency on `libc` (exact version of the lock).
- `crates/iris`: the supervisor thread, the requests `job.run` and
  `job.stop` on `iris.sock`, the session socket, the verbs `iris job run`,
  `iris job stop` and the hidden `iris session-hold`, and session mode.
- Spec 1 section 5.1: `sessions/` and `agents.toml` in the data folder.
- Spec 2 section 8.2: the verbs `iris job run`, `iris job stop` and
  `iris job session`.

Memory: the supervisor holds one small record for each running session
(the child handle, the pipes, the session ID, the egress listener). The
proxy uses two threads (one for each direction), two 16 KiB buffers and
a ClientHello buffer of at most 16 KiB for each open connection, at most
8 connections for each session, and at most 16 resolver threads in the
process. The helper reads the output of the
agent with one 16 KiB buffer. The helper is a second copy of the program, but it
only waits: its resident memory is small, and the build measures it
(open question 1).

## Assumptions

- `claude -p` runs one prompt and exits, and `--allowedTools` limits its
  tools. Evidence: pending. The owner checks the line on the Mac (issue). If
  the flags differ, only the adapter table changes.
- `codex exec --sandbox workspace-write` runs one prompt and exits, with no
  network. Evidence: pending, the same check. The same tolerance.
- `claude` and `codex` send their HTTPS calls through `HTTPS_PROXY` with
  `CONNECT`. Evidence: pending, the same check on the Mac. If a tool does
  not, it fails at its first model call and the egress log shows no
  connection. No mail leaves.
- The kernel closes the write end of the pipe when the engine process ends
  for any reason, also `SIGKILL`. Evidence: POSIX `exit` and `close` rules.
  The helper test checks it.
- `std::process::Command::process_group` exists on macOS and Linux.
  Evidence: Rust 1.64 `CommandExt::process_group`.

## External writes

The launcher writes nothing outside the product. The agent writes changes
to the mailbox only through the verbs, and no change reaches the mailbox
before `submit` (spec 2). `submit` gates changes to the mailbox only: the
agent tool can send the data of the workspace to an allowed host of its
adapter at any time of the session, before any `submit`.

The agent tool calls its model host through the egress proxy. Each call is
a request of the agent tool, on the account and the key of the agent tool,
not of Iris. The proxy relays bytes and makes no request of its own:

| Result | What happens |
|---|---|
| Success | The proxy relays the answer. The egress log line has the bytes. |
| A definite rejection (the host refuses the TLS connection or answers an error) | The proxy relays it, or closes both sides. The agent tool decides what to do. The egress log line has the result. |
| An unknown result (a time-out, a lost connection, an engine crash) | The proxy closes both sides. On an engine crash the kernel closes them, and the helper stops the agent. The provider can have taken and billed the request. |
| A retry | Iris never retries and never replays a request: it cannot read the TLS bytes. The agent tool can retry, by its own rules. |

The cost of these calls is on the account of the agent tool. The budget of
Iris (cost control) does not count it. The session time limit and the
limit of 8 connections bound it. This is a known limit with an issue.

## Open questions

1. The resident memory of `iris session-hold`. The build measures it and
   writes it in the PR.

## Acceptance criteria

1. `iris job run` with the generic adapter of a test script starts the
   script in the workspace, and the script sees `JOB.md`. Red if the script
   does not run, or runs in another folder.
2. A second `run` of the same job fails while the first session is open.
   Red if two sessions of one job are open.
3. `iris job stop` ends the session and exits with 0, and when it returns
   no process of the session runs, also when the script ignores `SIGTERM`.
   Red if it exits with 0 while a process of the session runs.
4. A revoke during the build ends the first session with `revoke` and its
   agent never starts; the launch registers a second session, builds with
   the new grant, and starts the agent there. The script writes its
   session ID and the thread files that it sees to a marker file. Red if
   the marker names the first session, or lists a thread of the revoked
   ref.
5. A revoke while the agent runs ends the session with `revoke`, and stops
   the group. Red if a process of the group runs 6 seconds later.
6. After a kill of the engine, the agent stops, and the next engine start
   ends the session with `engine`. Red if the agent runs or the row stays
   open.
7. The environment of the agent has no variable that starts with `IRIS`, and
   no variable outside the list. `HTTPS_PROXY` and `https_proxy` are there
   only for an adapter with allowed hosts, with the port of the session.
   Red if the script finds another variable, or a proxy variable with no
   allowed host.
8. In session mode, `iris search` does not open the store, and finds only
   granted messages: a word of a message outside the grant gives no
   result. Red if it opens the store or prints a message that is not
   granted.
9. With no isolation, `iris job run` fails before a session row is written.
   Red if a row or a process appears.
10. After `iris job stop`, a second `run` fails until the processes of the
    first session stopped, then succeeds. Red if a build starts while a
    process of the first session runs.
11. An agent that exits and leaves `sleep 100 &` has no running process 6
    seconds later. Red if the `sleep` runs.
12. After the helper is killed while the agent group runs, the session
    stays live and a `run` fails, until the group is empty. Red if
    `reaped_at` is set while a process of the group runs.
13. Through the egress port, a `CONNECT` to an allowed host reaches a test
    server (the test gives the proxy a resolver that maps the host to the
    test server), and a `CONNECT` to another host gets `403`. Red if the other
    host is reached.

## Verify

The tests run the generic adapter with test scripts in the test isolation.

```bash
cargo test -p iris-job
cargo test -p iris-session
cargo test -p iris
```

## Rollout

Migration 21 adds one table. The verbs are new. Nothing changes for a person
who does not run `iris job run`.

## Risks

- `iris job run` does nothing for the person until R4-05 merges (Linux) or R4-05m merges (macOS). This is
  on purpose: no agent runs without an isolation.
- A wrong adapter line makes the agent fail at once. The log shows the exit
  code, not the error text, because no agent output is stored. The person
  runs the adapter line by hand to see the error.
- Rollback: stop the engine, revert the code PRs, and restore the copy
  `iris.db.v<N>.<time>-<random>.bak` that the store wrote before the
  upgrade (spec 1, section 5.5). `<N>` is the version of the store before
  the upgrade, for example 19 if the store skipped version 20. Run a
  program that knows version `<N>`. A program that knows only version 20
  or lower cannot open a store of version 21, so the revert alone is not
  enough. Each change to the store after the upgrade is lost; the next
  sync gets the mail again.

## Plan

Each step is one card of the board, with one PR. Each card has `doneWhen`
and `dependsOn` on the board.

| Card | PR | Depends on |
|---|---|---|
| R4-04 | This spec. | R4-02 |
| R4-04b | Migration 21 and the session functions in `iris-job`. | R4-04 |
| R4-04c | The crate `iris-session`: adapters, `agents.toml`, the environment of the agent and the `Isolation` interface. | R4-04b |
| R4-04g | The hold helper in `iris-session`. | R4-04c |
| R4-04h | The launch and the reap check (`iris_session::reap::reap_check`) in `iris-session`, and the fix of #432. | R4-04g |
| R4-04d | The egress proxy in `iris-session`. | R4-04 |
| R4-04e | The engine sessions: the supervisor (it calls the reap check of R4-04h), the end of the open sessions at the engine start, the egress port of each session, `iris job run`, `iris job stop` and `iris job session`, and the hidden `iris session-hold`. | R4-04h, R4-04d |
| R4-04i | The session socket (`status`, and the verbs of R4-03 as the author of the session) and session mode of the command. It also changes the fixed rules of `JOB.md` in `iris-workspace` to the text of the workspace spec. | R4-04e |
| R4-04f | The read verbs `search`, `show` and `list` on the session socket. | R4-04i |

R4-05 (the isolation of Linux) and R4-05m (macOS) depend on R4-04c, R4-04d and R4-04g. The
cards that need a running session (R4-08, R4-11, R5-04) depend on R4-04e.

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
