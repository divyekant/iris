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
   R4-05 gives the isolation of macOS and of Linux. Until R4-05 merges,
   `iris job run` refuses each run.

## Not in scope

- The isolation of each system: no network other than the egress proxy, no
  data folder and no host socket (R4-05). This card gives the interface and refuses a run with no
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
| `helper_pid` | INTEGER | NULL until step 5 of the launch. The process ID of the hold helper. |
| `agent_pgid` | INTEGER | NULL until the helper starts the agent. The process group ID of the agent. |
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
- An insert needs a job in the state `working`, and NULL in `started_at`,
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
| `codex` | `codex exec --sandbox workspace-write <prompt>` |
| `<name>` from `agents.toml` | The `command` list of the file, then the prompt as the last argument. |

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
  engine has them. Each name is `[A-Z_][A-Z0-9_]*`. A name that starts with
  `IRIS` is refused.
- `allow_hosts` names the hosts that the agent can reach through the egress
  proxy, at most 16. Each one is a DNS name in lower case, with no port and
  no wildcard. An IP address is refused. The default is no host: the agent
  has no network.
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

The log never holds a secret value that the session passed. Before each
write, the helper replaces each value of a passed secret variable with
`[hidden]`. The secret variables are the keys of the built-in adapters
(`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) and each `pass_env` variable. A
value shorter than 8 bytes is refused at the run, because a short value
can hide normal text. The filter keeps the last bytes of each chunk (the
length of the longest value, less one) until the next chunk, so a value
split across two reads is also found. An encoded form of a value (for
example Base64) is not found: this is a known limit with an issue.

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
     Then the helper writes one line `started <pgid>` on its standard
     error, which is a second pipe to the engine. The engine writes
     `agent_pgid` (the spawn move).
   - Any other line, or the end of the pipe: the helper exits with no child.

   The helper itself is started inside the outer part of the isolation, so
   the isolation binds the life of each process of the session to the
   helper (see [Isolation](#isolation), lifetime rule).
3. After the start, the helper waits for two events: the child exits, or
   the pipe ends (the engine stopped, or the engine closed the pipe to stop
   the session).
4. On each of the two events, the helper stops the group of the agent:
   `SIGTERM` to the group, a wait of up to 5 seconds, then `SIGKILL` to the
   group. Then it calls `Isolation::stop` of the session, which ends each
   process that left the group. It does this also after a normal exit of the agent, so a
   background child that the agent left in its group stops too. Then the
   helper reaps the agent, and exits with the exit code of the agent (or
   the code of the signal).

So the pipe ties the session to the engine: when the engine process ends for
any reason, the kernel closes the pipe, and the helper stops the agent. The
helper waits for the group with `kill(-pgid, 0)`: the group has a process
while the call succeeds. A process that the agent moves out of its group
(`setsid`) is stopped by `Isolation::stop`, which the helper calls on the
end of the pipe with no engine. If the helper is killed too, the lifetime
rule of the isolation ends each process of the session. So no process of a
session outlives both the engine and the helper.

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
5. Start the hold helper (held), and write `helper_pid` (the helper move)
   in the same step. If the write fails, close the pipe and wait for the
   helper.
6. Start: `iris_job::start` in a write transaction, only if the session did
   not end. If it ended (a revoke or a job move came during steps 3 to 5),
   close the pipe, wait for the helper, and go back to step 2 once. A second
   end of the same kind ends the launch with the reason.
7. Write `start` to the pipe.

Each failure after step 2 ends the session with `launch_failed`, closes the
pipe, waits for the helper if one started, runs the reap check and removes
the socket. So no live session stays without a process. In step 6, a session
that ended is reaped the same way before step 2 runs again.

### Supervisor

The engine keeps one supervisor thread for all sessions. Each second, and
after each request on the engine socket that can end a session, it checks
each running session:

- The helper exited: reap it, end the session with `exit` and its exit
  code if it is still open, then run the reap check. Then remove the socket
  file. If the job is `done`, build its workspace again after the reap.
- A session that ended but did not pass the reap check: run the reap check
  again. The session stays live until the check passes.
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

The engine sets `reaped_at` only when it confirms that each process of the
session ended. The check passes only if all of these are true:

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

### Isolation

An isolation runs the agent so that it cannot reach the data folder, the
engine socket `iris.sock`, the socket of another session, or the network.
The one exception is the egress port of the session (see
[Model egress](#model-egress)).
It shows the agent only the workspace (read and write), the programs of the
system and the folders that the adapter needs (read only, for example the
settings of the agent tool). It ends each process in it when the session
ends.

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
  kernel kills each process in it. R4-05 gives the mechanism of each
  system, and an isolation that cannot keep this rule does not pass the
  check of the system.

The engine calls `stop` on each end of a session, after it closes the pipe:
on a stop, a revoke, a job move, a time limit, an exit of the agent, a
failed launch, and at the engine start for each live session. The
supervisor calls `stop` again on each turn while the reap check fails. So
no process of a session runs past its end, also when the helper is gone.

This card gives no isolation for a real system: R4-05 gives one for macOS and one for Linux. With no
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
- The agent gets `HTTPS_PROXY=http://127.0.0.1:<port>` (and `https_proxy`).
- The proxy takes only `CONNECT <host>:443`. The host must be in the
  allowed hosts of the adapter: `api.anthropic.com` for `claude-code`,
  `api.openai.com` for `codex`, and `allow_hosts` for an adapter of
  `agents.toml`. The engine resolves the host itself. Each other request,
  host or port gets `403` and closes.
- The proxy relays the bytes and does not read them. TLS runs from the
  agent to the model host, so the engine sees no key and no text.
- The proxy writes one line for each connection to its own file,
  `<data dir>/sessions/se-<id>.egress.log` (mode `0600`): the host, the
  bytes in each direction, and the result. Only the proxy writes this file,
  so it needs no shared append path with the helper. The file has a limit
  of 1 MiB. At the limit the proxy writes one line that says the rest is
  dropped, then only counts: at the end of the session it writes one last
  line with the count of dropped lines, inside a reserved 4 KiB. So the
  two logs of a session never pass 10 MiB and 1 MiB.
- When the session ends, the engine closes the port and each open
  connection of the session.

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
  search runs only over the messages of `job_scope_message` of the active
  refs of the job, and adds nothing to the workspace. `show` and `list`
  give only the items of the workspace, with the fields of the item file.
  The engine reads the grant at each call, so a revoke takes effect at
  once. These verbs come in their own PR (plan, PR 6). Until it merges,
  the socket refuses them with exit code 3 and the reason.

R4-03 adds the engine side of the other
session verbs: one entry point that takes the request and the author of the
session (kind `agent`, `name` the adapter, `session` `se-<id>`, `job`
`j-<id>`) and applies the grant checks. When that entry point is on
`develop`, the session socket gives it each verb that is not in the list
above. Until
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
- Full network for the built-in adapters: the agent could then send mail
  text to any host. The proxy permits only the model host.
- A proxy that opens TLS to read requests: it needs a local certificate
  authority that the agent tool trusts, and it adds no rule that the host
  list does not give.
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
| A raw SQL update or delete of `job_session` outside the five moves | Refused by the triggers | Store test |
| A request on the session socket for another verb | Exit code 3, nothing changes | Socket test |
| An agent process that leaves the process group | `Isolation::stop` ends it on each end of the session; the reap check needs `ended` | Test isolation that records the calls; R4-05 test on each system |
| A done job with a live session at the engine start | The build waits for the reap check of the session | Supervisor test |
| A `search` in a session for a word of a message outside the grant | No result | Socket test |
| The helper is killed and the agent group lives | The reap check fails on rule 2; the session stays live and a `run` is refused | Supervisor test: kill the helper, keep the group |
| The engine restarts while the group still runs | The session stays live until the group is empty; no time sets `reaped_at` | Supervisor test with a group that ignores `SIGTERM` for 8 seconds |
| `kill` gives an error other than `ESRCH` | The check fails; the session stays live | Unit test of the check with a fake `kill` |
| A `CONNECT` to a host that is not allowed, or a port other than 443, or a request that is not `CONNECT` | `403`, closed, one egress log line | Proxy test |
| An agent that opens connections without end | The egress log stops at 1 MiB, then counts; the engine memory stays bounded (8 open connections) | Proxy test with 100,000 refused connections and a small limit |
| The engine is killed and an agent process left the group (`setsid`) | The helper sees the end of the pipe, stops the group and calls `Isolation::stop`; if the helper is killed too, the lifetime rule ends the process | Helper test with the test isolation that records `stop`; R4-05 test of the lifetime rule on each system |
| An agent that prints a passed key | The log has `[hidden]` in place of the value, also when a read splits the value | Helper property test: random chunk splits of output with the value |
| A `pass_env` value shorter than 8 bytes | The run is refused with the name | Adapter test |
| A connection to the egress port of another session | The other session is not reached: each port belongs to one session, and the isolation permits only its own port | R4-05 test |
| An `allow_hosts` entry with a port, a wildcard or an IP address | Refused with the line | Parser test |

## Impact

- `crates/iris-store/src/schema_20.sql`: the table and the triggers.
- `crates/iris-job`: the session functions.
- New crate `crates/iris-session`: adapters, `agents.toml`, the hold helper,
  the launch, the reap check and the egress proxy. A direct dependency on `libc` (exact version of the lock).
- `crates/iris`: the supervisor thread, the requests `job.run` and
  `job.stop` on `iris.sock`, the session socket, the verbs `iris job run`,
  `iris job stop` and the hidden `iris session-hold`, and session mode.
- Spec 1 section 5.1: `sessions/` and `agents.toml` in the data folder.
- Spec 2 section 8.2: the verbs `iris job run` and `iris job stop`.

Memory: the supervisor holds one small record for each running session
(the child handle, the pipes, the session ID, the egress listener). The
proxy uses one thread and two 16 KiB buffers for each open connection, at
most 8 connections for each session. The log copy uses a buffer of
64 KiB for each session. The helper is a second copy of the program, but it
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
  not, it fails at its first model call and the log shows the refused
  connection. No mail leaves.
- The kernel closes the write end of the pipe when the engine process ends
  for any reason, also `SIGKILL`. Evidence: POSIX `exit` and `close` rules.
  The helper test checks it.
- `std::process::Command::process_group` exists on macOS and Linux.
  Evidence: Rust 1.64 `CommandExt::process_group`.

## External writes

The launcher writes nothing outside the product. The agent writes changes
to the mailbox only through the verbs, and nothing leaves before `submit`
(spec 2).

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
4. R4-04 PR 4: the egress proxy in `iris-session`.
5. R4-04 PR 5: the engine: supervisor, reap check, requests, session
   socket, egress ports, verbs and session mode.
6. R4-04 PR 6: the read verbs `search`, `show` and `list` on the session
   socket.

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
