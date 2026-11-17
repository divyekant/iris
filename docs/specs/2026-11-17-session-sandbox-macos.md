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
   the workspace of the job; a write outside the workspace, the private
   home and the private `/tmp` of the session is refused.
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
   `launchctl bootstrap`. `<tag>` is the first 8 hex digits of the SHA-256
   of the real path of the data folder, so two data folders do not share a
   label. The job runs `iris session-job <data dir> <id>`, with
   `RunAtLoad` true and `KeepAlive` false.
2. The job process connects to the socket. The shim checks that the peer
   has the same user ID (`getpeereid`). It sends its file descriptors 0, 1
   and 2 (the pipe from the engine, the session log and the status pipe)
   and the hold arguments, with `SCM_RIGHTS`.
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

Before the job process starts the agent, it writes the record
`<data dir>/sessions/se-<id>.sandbox`, as on Linux (temporary file,
`fsync`, `link`, `fsync` of the folder). The record holds the boot time
(`sysctl kern.boottime`) and the resource coalition ID of the job process
(`proc_pidinfo`, `PROC_PIDCOALITIONINFO`). A coalition ID is not used again
in one boot. The agent runs only after the record exists.

`ended(session)`:

- No record: true. The agent did not start.
- The boot time differs: true. The system started again.
- `coalition_info_resource_usage` gives `ESRCH`: true. The coalition is
  gone, so it holds no process.
- `tasks_exited` equals `tasks_started`: true.
- Each other result, also an error: false.

`stop(session)`:

1. `launchctl bootout` of the job. A job that is not loaded is not an
   error.
2. Until `ended` is true or 5 seconds pass: scan the processes
   (`proc_listallpids`), and for each process in the coalition of the
   record send `SIGSTOP`, read its coalition again, and send `SIGKILL` if
   it is still in the coalition. Else send `SIGCONT` to it, because its
   process ID went to another process between the reads.
3. Return `Ok` if `ended` is true, else an error that names the count of
   live processes.

`name(session)` is the label of the job.

### The Seatbelt profile

`command()` gives `/usr/bin/sandbox-exec -p <profile> -- <program> <args>`.
The profile starts with `(deny default)` and allows only these:

| Operation | Allowed for |
|---|---|
| `process-exec`, `process-fork` | Each program that the profile lets the session read |
| `signal`, `process-info*` | `(target same-sandbox)` only |
| `sysctl-read` | All |
| `file-read*` | The system folders: `/usr` without `/usr/local`, `/bin`, `/sbin`, `/System`, `/Library/Apple`, `/private/var/db/dyld`; the device files `/dev/null`, `/dev/zero`, `/dev/random`, `/dev/urandom`; in `/private/etc` only `hosts`, `passwd`, `group`, `localtime` and the public certificate files (`ssl/cert.pem`, `ssl/certs`); the adapter program file and the `iris` program file (each one a literal real path); the workspace; the private home and `/tmp` of the session |
| `file-read-metadata` | The paths above, and each folder above them (literal paths), so that a program can resolve its paths |
| `file-write*` | The workspace, the private home and `/tmp` of the session, `/dev/null` |
| `network-outbound` | `(remote ip "localhost:<egress port>")` only, if the session has an egress port |

Each path in the profile is a real path (`canonicalize`). Seatbelt checks
real paths, and macOS links `/tmp` and `/etc` to `/private`.

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
not work there. The port stays with its session until the reap (launcher
spec), and the reap waits for `ended`, so a process of an old session
cannot reach the proxy of a new session.

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
| A process ID goes to another process between the scan and the signal | `SIGSTOP`, then the second read shows another coalition: `SIGCONT`, no `SIGKILL` | Unit test of the signal step with a process of another coalition |
| The agent reads the data folder, the home folder, `~/.ssh`, or another workspace | Refused | Mac acceptance 1 |
| The agent connects to another port, a public address, UDP or a Unix socket | Refused | Mac acceptance 1 |
| The agent calls `launchctl`, `open`, `pbpaste` or another system service | Refused (no `mach-lookup`) | Mac acceptance 1 |
| The agent signals a process outside the sandbox | Refused | Mac acceptance 1 |
| No GUI domain (no person signed in, for example an SSH session only) | `check` fails with the reason; `run` is refused | Unit test of the reason text |
| The job process does not connect in 1 second | The shim boots out the job and exits with 1 | Mac acceptance 4 |
| The engine crashes while a session runs | The pipe ends, the hold stops the agent; the new engine reads the record and calls `stop` and `ended` | Mac acceptance 4 |
| The system starts again | The boot time differs: `ended` is true | Unit test with a record of another boot time |
| The record cannot be written | `spawn` fails; the agent did not run | Unit test with a read-only sessions folder |
| A program file in a folder of the person (for example `~/.local/bin/claude`) | Only that file is readable | Mac acceptance 1 |

## Impact

- `crates/iris-session/src/isolation.rs`: `system()` on macOS, the trait
  text of the lifetime rule.
- New: `crates/iris-session/src/isolation_macos.rs` (profile, record,
  `stop`, `ended`, check) and `isolation_macos_tests.rs`.
- `crates/iris-session/src/hold.rs` or the helper entry of the program:
  the shim and the `session-job` verb on macOS.
- `crates/iris/src/sessions.rs`: the egress peer `127.0.0.1` on macOS.
- The launcher spec, "Isolation": the macOS form of the lifetime rule.
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
- The Claude Code native program and Codex run with no `mach-lookup`.
  Pending: Mac acceptance 2. If not, the profile gets the smallest list of
  services that the program needs, in the code PR, with the evidence.

## External writes

None. The isolation writes files in the data folder, loads and removes
launchd jobs of its own label, and sends signals to processes of the
coalition of the session.

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
2. R4-05m code: the macOS isolation, the shim and the job verb, the egress
   peer, and the Mac acceptance.
