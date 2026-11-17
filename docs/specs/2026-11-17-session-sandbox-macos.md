# Spec: the sandbox of a session on macOS

- Date: 2026-11-17
- Card: R4-05m (the macOS sandbox of a session)
- Status: Draft

## Problem

On macOS, `iris_session::isolation::system()` gives no isolation, so
`iris job run` is refused on the Mac (issue #508). The Linux spec
(`docs/specs/2026-11-17-session-sandbox.md`, section "macOS") gives the
reason: macOS has no public mechanism that stops each process of a session
with no race. A process can call `setsid` and fork and exit in a loop, and
a scan of the processes does not catch it.

This spec gives a macOS isolation. A probe on the owner's Mac gave the
evidence (see [Evidence](#evidence)).

## Goal

1. On macOS, `system(data_dir)` gives an isolation that passes its check.
2. Goals 2, 3 and 4 of the Linux spec hold on macOS: the network is only the
   egress port of the session; the data folder is not visible other than
   the workspace of the job and the private home and `/tmp` of the
   session; a write outside these three folders is refused. The private
   home and `/tmp` are in the data folder (`sessions/se-<id>.home` and
   `sessions/se-<id>.tmp`), because macOS has no `tmpfs` for a normal
   user. They are empty at the start, only this session sees them, and
   the reap removes them. No other path of the data folder is visible.
3. `ended(session)` is exact: it is true only when no process of the
   session runs, also a process that called `setsid`. It is never true
   while a process of the session runs.
4. The engine never reaps a session while `ended` is false. So a process of
   an old session never sees the workspace of a later session of the job,
   and never reaches the egress port of another session.

## Not in scope

- A stop with a time limit. See [The change of the lifetime rule](#the-change-of-the-lifetime-rule).
- Limits of CPU, memory or disk for a session.
- Programs that need files outside the system folders and their own file
  (the same limit as Linux, issue #505).

## Evidence

The probe `scripts/probes/macos-sandbox-probe.py` ran four times on the
owner's Mac (macOS 26.6.2, arm64) on 2026-11-17. The results:

| Question | Result |
|---|---|
| Does a `deny default` Seatbelt profile block the data folder, the home folder, `~/.ssh`, `/tmp`, other loopback ports, public TCP and UDP, Unix sockets, `ps`, signals to other processes, `launchctl submit` and the pasteboard? | Yes, each one. A write to the workspace, a connection to the egress port and a program run worked. |
| Does a launchd job get its own resource coalition, and do its children stay in it after `setsid`? | Yes. Each job got a new coalition ID. A child that called `setsid` twice stayed in it. |
| Does `coalition_info_resource_usage` work for a normal user? | Yes. It gives `tasks_started` and `tasks_exited`. It gives `ESRCH` when the coalition is gone. |
| Does `launchctl bootout`, or a `SIGKILL` of the job process, stop the coalition? | No. In 12 of 12 runs, a child that called `setsid` lived on. |
| Does a scan of the coalition members and a `SIGKILL` of each one stop a process that forks and exits in a loop? | Not always. In 6 of 10 runs, a loop of scans stopped it within 5 seconds. A single scan stopped it in 0 of 7 runs. |
| Does `kill(-1, SIGKILL)` inside the sandbox stop the processes of the sandbox? | No. The call returns 0, but no process gets the signal (10 of 10 runs). No process outside the sandbox got a signal. |

## Design

### The change of the lifetime rule

The Linux rule: when the helper ends for any reason, each process of the
session ends, with no call from the engine. macOS has no mechanism for this
rule (evidence above). On macOS the rule changes to this:

- `ended(session)` is exact, from the counters of the kernel.
- `stop(session)` stops each process that it finds. If a process still
  runs after its time limit, `stop` returns an error. The reap check then
  fails, and the engine tries again at its next turn (launcher spec, "Reap
  check").
- The engine reaps a session only when `ended` is true. Until then, the
  session keeps its workspace and its egress port, and the job starts no
  new session.

So a process that escapes a stop can use CPU until a later stop catches
it. It cannot read a later session's data, and it cannot reach another
session's egress port. The `Isolation` trait text changes to say this.

### The launchd job of a session

The helper must run in a coalition of its own, so that `ended` counts only
the processes of the session. Only launchd makes a new coalition for a
normal user. The engine and the launcher stay the same (the helper is
still a child of the engine, with the same pipes):

1. The engine starts `iris session-hold <data dir> <hold args>`, as on
   Linux. On macOS this process is the "shim". It makes the socket
   `<data dir>/sessions/se-<id>.hold` (mode `0600`) and loads the launchd
   job `com.iris.session.<tag>.<id>` in the domain `gui/<uid>` with
   `launchctl bootstrap`. `<tag>` is the full SHA-256, in 64 hex digits,
   of the real path of the data folder, so two data folders do not share a
   label. The job runs `iris session-job <data dir> <id>`, with
   `RunAtLoad` true and `KeepAlive` false.
2. The job process first writes the record of the session (see
   [The record of a session](#the-record-of-a-session)). Then it connects
   to the socket. The shim checks that the peer has the same user ID
   (`getpeereid`) and that the record exists and names the coalition of
   the peer process (`LOCAL_PEERPID`, then `PROC_PIDCOALITIONINFO`). Only
   then it sends, with `SCM_RIGHTS` on the socket:
   - its file descriptors 0, 1 and 2 (the pipe from the engine, the
     session log and the status pipe);
   - a descriptor of its working folder (the workspace, which the launch
     set);
   - its whole environment (the agent environment of the launch, with the
     key variable and the `HTTPS_PROXY` value with the token);
   - the hold arguments.

   A launchd job does not inherit the environment or the working folder
   of the shim, so this handoff is their only path. The job definition of
   launchd holds no variable of the agent, and nothing of the handoff goes
   to a log. The job process clears its own environment (launchd sets some
   variables), sets the received one, and calls `fchdir` on the received
   folder. Then it runs the hold helper. The socket is in the sessions
   folder (mode `0700`), and the shim checks the user and the coalition of
   the peer first, so only the job of this session gets the key and the
   token.
3. The job process runs the hold helper (`hold`) with these descriptors.
   So the protocol of the helper does not change.
4. The shim waits for the end of the socket. The job process writes its
   exit code to the socket before it exits. The shim exits with that code,
   or with 1 if no code came. So the engine reaps the shim as it reaps the
   helper on Linux.
5. If the socket ends before the code (the shim or the job process ended),
   the job process stops the agent (as for the end of the engine pipe) and
   exits.

The shim waits at most 1 second for the connection. If no job connects,
the shim boots out the job and exits with 1 (`failed`).

### The record of a session

The job process writes the record before it connects to the shim, so
before the handoff of step 2. It writes the record
`<data dir>/sessions/se-<id>.sandbox`, as on Linux (temporary file,
`fsync`, `link`, `fsync` of the folder). The record holds the boot time
(`sysctl kern.boottime`) and the resource coalition ID of the job process
(`proc_pidinfo`, `PROC_PIDCOALITIONINFO`), and the egress port of the
session (or none). A coalition ID is not used again in one boot. The reap
removes the record only after `ended` is true.

Invariant: if no record exists when the engine reaps the session, no
agent of the session runs or can start later. The agent starts only after
the handoff. The handoff needs a live shim, and the shim checks the record
first. The reap check needs the end of the shim (launcher spec). So with
no record at the reap, the handoff did not happen and cannot happen. A job
process that writes its record after that reap finds no shim and exits
with no agent. Its record names a coalition that ends with it, so a later
`ended` gives true.

`ended(session)`:

- No record: true. No agent ran, and none can start (invariant above).
- The boot time differs: true. The system started again.
- `coalition_info_resource_usage` gives `ESRCH`: true. The coalition is
  gone, so it holds no process.
- `tasks_exited` equals `tasks_started`: true.
- Each other result, also an error: false.

`stop(session)`:

`stop` holds the lock file `<data dir>/sessions/se-<id>.stop.lock`
(`iris_store::FileLock`, an exclusive `flock`) for its whole run: the
journal recovery, each signal and each journal write. The helper and the
engine both call `stop`, so the lock serializes them. A second caller
waits for the lock, then sees the result of the first.

0. If the caller is in the coalition of the record (the hold helper in
   the job process calls `stop` after the agent exits), return `Ok` and do
   nothing. A process cannot freeze its own coalition: it would stop
   itself while it holds the lock, and `bootout` would end it. The
   `stop` of the engine, which runs outside the coalition, does the work
   (see below). The `ended` of the engine stays false while the helper
   runs, so this early `Ok` does not let the reap pass.
1. `launchctl bootout` of the job. A job that is not loaded is not an
   error. The label holds the full SHA-256 of the data folder and the
   session ID, so `bootout` never acts on a job of another data folder or
   another session.
2. Until `ended` is true or 5 seconds pass, do a freeze, then a kill:
   1. Freeze: scan the processes (`proc_listallpids`). For each process
      in the coalition of the record, send `SIGSTOP`, then read its
      coalition again. If the process is not in the coalition now (its
      process ID went to another process between the reads), send
      `SIGCONT` to it. Before each `SIGSTOP`, append the process ID to the
      journal
      `<data dir>/sessions/se-<id>.stops` (with `fsync`). Repeat the scan until one scan finds each live
      member stopped (`pbi_status` is `SSTOP`), the count of these
      members equals `tasks_started - tasks_exited`, and `tasks_started`
      did not change during the scan. Then no member of the coalition can
      run, fork or send a signal.
   2. Kill: for each member of the frozen set, read its status, its
      coalition and its unique ID (`PROC_PIDUNIQIDENTIFIERINFO`) again,
      and send `SIGKILL` only if it is still stopped, in the coalition and
      has the same unique ID. A stopped process cannot exit by itself, and
      no member runs to signal it.
      Invariant: no process of the session can make `stop` send a signal
      to a process outside the session. Each member is stopped, and the
      profile denies signals outside the sandbox.

   Recovery of the journal: at the start of each `stop` (also the `stop`
   of a new engine after a crash), and before `stop` returns, `stop`
   resolves each entry of the journal of the session:

   | State of the entry | Result |
   |---|---|
   | No process has the process ID | Resolved: no process can be stopped by the entry |
   | The process is in the coalition of the session | Resolved: it is a process of the session |
   | Another coalition, and its status is not `SSTOP` | Resolved: it runs |
   | Another coalition, stopped | Send `SIGCONT`, then read the status again. Not `SSTOP`: resolved. Else, or if the signal or a read fails: not resolved |

   The recovery reads the process that has the process ID now, not the
   process of the journal time. The process ID can go to another process
   between the journal write and the `SIGSTOP`, and then that other
   process is the one that Iris can have stopped. So each stopped process
   with a journaled process ID that is not in the session gets `SIGCONT`.

   `stop` rewrites the journal with the entries that are not resolved
   (temporary file, `fsync`, `rename`). It returns `Ok` only if `ended`
   is true and the journal is empty. Else it returns an error, so the
   reap check fails and the engine tries again at its next turn. The reap
   removes the journal file only after `stop` returned `Ok`. So no
   process that Iris stopped by mistake stays stopped with no record, also
   after a crash between the `SIGSTOP` and the `SIGCONT`. A process that
   the person had stopped on purpose, and whose process ID Iris read in
   the same moment, continues: see the known limit below.

   macOS gives no signal by process identity (no `pidfd`). So one race
   stays, which the session cannot cause: between the last read and the
   `SIGKILL`, a process outside the sandbox kills the stopped member,
   launchd reaps it, and the system gives the same process ID to a new
   process. The window is one system call. Known limit (issue).
3. Return `Ok` only if `ended` is true and the journal is empty (see the
   recovery of the journal below). Else return an error that names the
   count of
   live processes.

`name(session)` is the label of the job.

### The Seatbelt profile

`command()` gives `/usr/bin/sandbox-exec -p <profile> -- <program> <args>`.
The profile starts with `(deny default)` and allows only these:

| Operation | Allowed for |
|---|---|
| `process-exec`, `process-fork` | Each program that the profile lets the session read |
| `signal`, `process-info*` | `(target same-sandbox)` only |
| `sysctl-read` | Only a list of names that tell the system and the hardware: the prefixes `hw.` and `machdep.cpu.`, and the names `kern.ostype`, `kern.osrelease`, `kern.osversion`, `kern.osproductversion`, `kern.version`, `kern.hostname`, `kern.boottime`, `kern.argmax`, `kern.maxfiles`, `kern.maxfilesperproc`, `kern.usrstack64`, `kern.secure_kernel`, `vm.loadavg`, `vm.pagesize` and `sysctl.proc_translated`. Never `kern.proc`, `kern.procargs` or `kern.procargs2`, which give the arguments and the environment of other processes |
| `file-read*` | The system folders: `/usr` without `/usr/local`, `/bin`, `/sbin`, `/System/Library`, `/System/Cryptexes`, `/System/Volumes/Preboot/Cryptexes`, `/Library/Apple`, `/private/var/db/dyld` (never all of `/System`: `/System/Volumes/Data` holds the home folders and the data folder); the device files `/dev/null`, `/dev/zero`, `/dev/random`, `/dev/urandom`; in `/private/etc` only `hosts`, `passwd`, `group`, `localtime` and the public certificate files (`ssl/cert.pem`, `ssl/certs`); the adapter program file and the `iris` program file (each one a literal real path); the workspace; the private home and `/tmp` of the session |
| `file-read-metadata` | The paths above, and each folder above them (literal paths), so that a program can resolve its paths |
| `file-write*` | The workspace, the private home and `/tmp` of the session, `/dev/null` |
| `network-outbound` | `(remote tcp "localhost:<egress port>")`, if the session has an egress port: TCP only, so a UDP datagram to the same port number (for example to another local service on that UDP port) is refused; and `(remote unix-socket (path-literal "<workspace>/.iris/socket"))`, the session socket (launcher spec, goal 6), which each `iris` verb of the session uses. No other Unix socket |

Each path in the profile is a real path (`canonicalize`). Seatbelt checks
real paths, and macOS links `/tmp` and `/etc` to `/private`. The last rule
of the profile is `(deny file-read* file-write* (subpath
"/System/Volumes/Data"))`, so the second path of the user volume never
gives access, also if an allowed folder is under it. The workspace and the
private folders are then allowed again by their own path after this rule:
Seatbelt takes the last rule that matches, and the workspace path that
Seatbelt sees is the real path under `/Users` or `/private`, not the
`/System/Volumes/Data` path.

The private home is `<data dir>/sessions/se-<id>.home` and the private
`/tmp` is `<data dir>/sessions/se-<id>.tmp`, both mode `0700`. The job
process makes them before the agent starts. The reap removes them after
`ended` is true. `HOME` and `TMPDIR` point to them.

The profile has no `mach-lookup` and no `iokit-open`. So the agent cannot
reach launchd, the pasteboard, the keychain or another system service.

### Egress on macOS

There is no network namespace, so no relay. The agent connects to
`127.0.0.1:<egress port>` directly, and the profile permits only that port.
The proxy of the session takes clients from `127.0.0.1` on macOS: the
engine passes `Ipv4Addr::LOCALHOST` as the peer (`sessions.rs`). macOS has
only `127.0.0.1` on the loopback interface, so `session_peer` addresses do
not work there.

Invariant: a process of a session reaches only the proxy of its own
session, also after an engine crash and with another data folder. A port
check cannot give this: after a crash, a new engine (perhaps of another
data folder) can get the port of a session that is not reaped. So each
proxy on macOS needs a secret of its session:

- The launch makes a random token of 32 bytes for each session, with the
  egress port, and gives it to the agent only in its environment:
  `HTTPS_PROXY=http://iris:<token>@127.0.0.1:<port>`. The token is not in
  the record, the store or a log.
- `Proxy::start` gets an optional token. With a token, the proxy answers
  each `CONNECT` with no `Proxy-Authorization: Basic` header that holds
  `iris:<token>` with `407` and the log word `not_session`, before it
  opens a connection. The compare takes constant time.
- A process of session A has only the token of A. The profile denies
  `process-info` and signals outside the sandbox, so it cannot read the
  environment of session B.

So the proxy of B refuses each process of A before any byte goes out, with
no timing assumption. The Linux proxy keeps the peer check and gets no
token.

### Check

`check()` passes if all of these work, in 5 seconds:

- `/usr/bin/sandbox-exec` runs `/usr/bin/true` with the profile of a test
  session.
- `launchctl print gui/<uid>` exits with 0 (the GUI domain exists: the
  person is signed in).
- `coalition_info_resource_usage` of the coalition of the engine gives a
  result.

## Options not chosen

- A Linux virtual machine on the Mac (Apple Virtualization or the
  Containerization framework) with the Linux sandbox inside: a stop with
  no race, but a virtual machine for each session costs hundreds of MB,
  needs Linux builds of the agent tools, and needs an image download.
- Stay Linux only: no agent sessions on the Mac.
- `kill(-1)` from a process inside the sandbox: the probe showed that it
  reaches no process.
- A launchd job with no scan: `bootout` does not stop the coalition.
- `sandbox-exec` with a scan of all processes and `sandbox_check` (the
  first design): no exact `ended`.
- The Anthropic `sandbox-runtime` profile: it reads the whole disk minus a
  deny list.
- Public libraries checked for the profile and the process code: the Rust
  crates `birdcage` (applies `sandbox_init` to the whole calling process,
  with allow-list exceptions but no rule for one TCP port or one Unix
  socket) and `gaol` (not maintained); `libproc` (no coalition flavor of
  `proc_pidinfo`). Iris writes the profile text itself and runs it with
  `sandbox-exec`, so the helper stays outside the sandbox, and calls
  `proc_pidinfo` and `coalition_info_resource_usage` through `libc`, a
  dependency of the crate already.
- Endpoint Security, `coalition_create` or `pid_suspend`: each one needs
  root or an Apple entitlement.

## Failure list

Supported input: the session ID, the workspace, the egress port and the
adapter that the launch gives; a program that the agent runs inside the
sandbox. Each other condition fails closed: `check` fails, `spawn` fails
before the agent runs, or `ended` is false.

| Input or event | Result | Test |
|---|---|---|
| The agent calls `setsid` and forks and exits in a loop | `ended` stays false until no process runs; the reap waits | Mac acceptance 3 |
| `stop` does not catch the loop in 5 seconds | `stop` returns an error; the engine tries again at its next turn; the session keeps its port and workspace | Mac acceptance 3 |
| The agent reads the data folder, the home folder, `~/.ssh`, or another workspace | Refused | Mac acceptance 1 |
| The agent reads the store key or a home file through `/System/Volumes/Data/...` | Refused | Mac acceptance 1 |
| The job of a session starts with the environment of launchd, not of the launch | The handoff gives the agent environment and the workspace; the job sets both before the hold | Mac acceptance 2 (the tool finds its key and its token, and runs in the workspace) |
| The engine crashes between a `SIGSTOP` to a reused process ID and the `SIGCONT` | The journal names the process; the next `stop` sends `SIGCONT` to it | Unit test: a journal entry of a stopped process of another coalition gets `SIGCONT` |
| `SIGCONT` to a journal entry fails, or the process stays stopped | The entry stays; `stop` returns an error and the reap waits | Unit test: an entry whose read fails stays in the journal, and `stop` gives an error |
| The agent reads the arguments or the environment of another process (`sysctl kern.procargs2`, `ps`) | Refused | Mac acceptance 1 |
| The hold helper calls `stop` from inside the coalition | `Ok` with no action; the stop of the engine, outside the coalition, freezes and kills, the helper too | Unit test: `stop` from a process in the coalition of the record sends no signal |
| The shim ends after the job process got the descriptors, before the agent starts | The record exists (it comes before the handoff), so `ended` reads the coalition | Unit test: the shim refuses the handoff with no record |
| The agent connects to another port, a public address, UDP or a Unix socket other than its session socket | Refused | Mac acceptance 1 |
| The agent runs `iris status` (the session socket `.iris/socket`) | Works | Mac acceptance 1 |
| The agent sends UDP to `127.0.0.1:<egress port>` | Refused: the rule allows TCP only | Mac acceptance 1 |
| The agent calls `launchctl`, `open`, `pbpaste` or another system service | Refused (no `mach-lookup`) | Mac acceptance 1 |
| The agent signals a process outside the sandbox | Refused | Mac acceptance 1 |
| No GUI domain (no person signed in, for example an SSH session only) | `check` fails with the reason; `run` is refused | Unit test of the reason text |
| The job process does not connect in 1 second | The shim boots out the job and exits with 1 | Mac acceptance 4 |
| The engine crashes while a session runs | The pipe ends, the hold stops the agent; the new engine reads the record and calls `stop` and `ended` | Mac acceptance 4 |
| The system starts again | The boot time differs: `ended` is true | Unit test with a record of another boot time |
| The record cannot be written | `spawn` fails; the agent did not run | Unit test with a read-only sessions folder |
| The engine crashes while an escaped process of session A lives, and a new engine (of the same or another data folder) gets the old port of A for the proxy of session B | The proxy of B answers `407` (`not_session`): the process has the token of A | Proxy test: no header, a wrong token and the token of another session each get `407`; the right token gets `200` |
| A process ID goes to another process during `stop` | The freeze reads the coalition after `SIGSTOP` and sends `SIGCONT` to a process of another coalition; `SIGKILL` goes only to the frozen set | Unit test of the freeze with a process of another coalition: it gets `SIGCONT` and lives |
| A program file in a folder of the person (for example `~/.local/bin/claude`) | Only that file is readable | Mac acceptance 1 |

## Impact

- `crates/iris-session/src/isolation.rs`: `system()` on macOS, the trait
  text of the lifetime rule.
- New: `crates/iris-session/src/isolation_macos.rs` (profile, record,
  `stop`, `ended`, check) and `isolation_macos_tests.rs`.
- `crates/iris-session/src/hold.rs` or the helper entry of the program:
  the shim and the `session-job` verb on macOS.
- `crates/iris/src/sessions.rs`: the egress peer `127.0.0.1` and the token
  of each session on macOS.
- `crates/iris-session/src/egress.rs`: the optional token of
  `Proxy::start` and the `407` answer.
- `crates/iris-session/src/config.rs` or the launch: the token in
  `HTTPS_PROXY` of the agent environment.
- The launcher spec, "Isolation", goal 3 and the failure list: the macOS
  exception to the lifetime rule (changed in this PR).
- The Stop spec (`docs/specs/2026-11-17-stop.md`): what runs while the
  engine is down on macOS (changed in this PR).
- `docs/DEPLOYMENT.md`: the Mac needs a signed-in user (GUI domain).

Memory: the shim is one more `iris` process for each session (a few MB)
while the session runs. No virtual machine.

## Assumptions

- Seatbelt (`sandbox-exec`) stays in macOS. Evidence: it is in macOS 26.6.2,
  marked deprecated since macOS 10.12; Codex and Claude Code use it. If
  Apple removes it, `check` fails and no agent runs.
- launchd gives each job in `gui/<uid>` a new resource coalition, and the
  children of the job stay in it. Evidence: the probe (19 jobs).
- A coalition ID is not used again in one boot. Evidence: the probe saw
  only new, larger IDs. Pending a source in the XNU code. If it is wrong,
  `ended` can be false for too long (fail closed), never true too early,
  because a new coalition with the same ID holds processes.
- `coalition_info_resource_usage` stays callable by a normal user.
  Evidence: the probe. If not, `check` fails.
- Claude Code (Node.js) and Codex send `Proxy-Authorization: Basic` from
  the user and password in `HTTPS_PROXY`. Evidence: the Claude Code
  network configuration documentation gives `http://user:pass@host:port`;
  `reqwest`, the HTTP client of Codex, takes the credentials of a proxy
  URL. Pending: Mac acceptance 2. If a tool does not send them, its
  session gets `407` and no data leaves (fail closed).
- The Claude Code native program and Codex run with no `mach-lookup`.
  Pending: Mac acceptance 2. If not, the profile gets the smallest list of
  services that the program needs, in the code PR, with the evidence.

## External writes

The isolation loads and removes launchd jobs (state of launchd outside the
data folder). It also writes files in the data folder and sends signals to
processes of the coalition of the session.

The label of a job is fixed by the data folder and the session ID, and a
session ID is never used again. So each retry and each recovery acts on
the same label, and a job of one session never acts on another session.

| Write | Success | Definite rejection | Unknown result (time-out, crash, lost answer) | Retry |
|---|---|---|---|---|
| `launchctl bootstrap` (the shim) | The job process connects; the hold runs | The shim exits with 1 (`failed`); `launch_failed` | The shim waits at most 1 second for the connection, then runs `bootout` and exits with 1. If the shim crashed, the job process writes its record, finds no shim at the socket, and exits with no agent. The record stays, and names a coalition that ends with the job process: `ended` gives true, and the reap of the session removes the record. If the reap came before the late record, the record stays on disk: it holds no secret, its session ID is never used again, and its coalition is gone, so each later `ended` gives true. The job can stay loaded with no process | Never: the launch does not retry a bootstrap. The reap calls `stop`, and `stop` runs `bootout` first |
| `launchctl bootout` (`stop`) | The job is removed | "No such process" (the job is not loaded): taken as success | `stop` does not depend on it: `ended` reads the coalition, not launchd | Each `stop` runs `bootout` again, and the reap calls `stop` until the check passes. A new engine reaps each open session at its start (launcher spec), so it removes the job of each session that a crash left |

## Open questions

1. Which `mach-lookup` services, if any, does each agent tool need?
2. How long does `stop` take on average against a fork loop on the Mac?

## Acceptance criteria

These run on the owner's Mac, because CI is Linux only.

1. In a session, each read, write, connection and service of the failure
   list is refused, and the workspace and the egress port work. Red if one
   refused action works.
2. A session of `claude-code` with a test task ends with the exit code 0.
   Red if the tool fails in the sandbox.
3. A session whose agent runs the fork loop of the probe: `ended` is false
   while the loop runs. The reap waits until `stop` caught the loop. Red
   if `ended` is true while a process of the loop runs.
4. A session after an engine crash: the new engine stops it and reaps it.
   Red if a process of the session runs after the reap.
5. On Linux, nothing changes. Red if a Linux sandbox test fails.

## Verify

```bash
cargo test -p iris-session --lib isolation
scripts/local-ci/run.sh
# On the Mac (owner):
cargo test -p iris-session --lib isolation_macos -- --include-ignored
```

## Rollout

None. `iris job run` starts to work on the Mac when the code PR merges.
Close issue #508.

## Risks

- A hostile agent that forks in a loop can hold one CPU core until a stop
  catches it. Known limit (issue).
- `SIGSTOP` can reach another process of the person for a moment, if its
  process ID was reused between two reads. The `SIGCONT` follows at once.
  A process that the person stopped on purpose (for example with Ctrl-Z)
  then continues. Known limit (issue).
- `sandbox-exec` is deprecated. Rollback: make `system()` return the error
  again on macOS.

## Plan

1. R4-05m spec: this file.
2. R4-05m code, in three PRs, so that each one stays small:
   1. The egress token and peer (PR #611).
   2. The isolation logic (`isolation_macos.rs`): the profile, the
      record, `ended`, `stop` with the freeze and the journal, and
      `check`, with the real process facts (`isolation_macos_sys.rs`).
      `system()` still refuses macOS after this PR, because no code makes
      the launchd job and the record yet.
   3. The shim, the `session-job` verb and the handoff, the private home
      and `/tmp`, their removal at the reap, and `system()` on macOS.
3. The Mac acceptance, after the third code PR.
