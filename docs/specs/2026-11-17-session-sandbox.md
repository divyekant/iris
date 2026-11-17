# Spec: the sandbox of a session on Linux

- Date: 2026-11-17
- Card: R4-05 (sandbox for sessions on macOS and Linux: no network and no
  credentials). This spec splits the card: R4-05 delivers Linux, and the
  new card R4-05m (todo, on the board) delivers macOS.
- Status: Draft

## Problem

The session launcher (`docs/specs/2026-11-17-session-launcher.md`, sections
"Isolation" and "Model egress") starts an agent only through an
`Isolation`. `iris_session::isolation::system()` gives none yet, so
`iris job run` is refused on each system. The launcher spec gives the rules
of an isolation. It leaves the mechanism of each system to this card.

The agent reads mail text that the product does not control. A message can
tell the agent to read the store key, to call the engine socket, or to send
the workspace to a host of the sender. The isolation is the only part that
stops these actions when the agent follows such a message.

## Goal

1. On Linux, `system(data_dir)` gives an isolation that passes its check
   on a supported system, and a reason for the person on a system that is
   not supported. On macOS and each other system it gives the reason (see
   [macOS](#macos)).
2. A process of a session cannot open a network connection, other than to
   the egress port of its own session.
3. A process of a session cannot read or write the data folder, other than
   the workspace of its own job. So it cannot read the store key, the
   database, the engine socket `iris.sock` or the socket of another
   session.
4. A process of a session cannot write a file outside its workspace, its
   private home and its private `/tmp`. The private home and `/tmp` are
   empty `tmpfs` mounts of the session: no host file is in them, and they
   are gone when the session ends. So a process cannot change a host file,
   for example a settings file that a program outside the sandbox runs
   later (the hooks of an agent tool).
5. No process of a session outlives its helper (the lifetime rule).
6. `stop` and `ended` find the processes of a session after a restart of
   the engine.

## Not in scope

- A sandbox for macOS, Windows or another system. `system()` gives the
  reason. Card R4-05m designs and builds the macOS sandbox (see
  [macOS](#macos)).

### Change of the platform scope

Iris runs on macOS and on Linux (AGENTS.md, "Base decisions"). This spec
changes that rule for one flow only: until R4-05m is done, an agent
session (`iris job run`) runs only on Linux. On macOS each other part of
Iris runs, and `iris job run` stops with the reason of `system()`. No
agent runs on the Mac with a sandbox that can let a process of a session
outlive its reap. The owner decides on the decision card of this thread
whether the Mac gets a weaker sandbox earlier. The `doneWhen` of R4-05m
is the Linux acceptance of this spec, on macOS.
- Limits of CPU, memory or disk for a session.
- A filter of the files that the agent sends to its own model host. The
  person chose that host (launcher spec, "Model egress").
- The settings files of the agent tool of the person (`~/.claude`,
  `~/.codex`). The session does not see them (see [Files](#files)).
- The wiring of the engine (R4-04e). This card changes only
  `crates/iris-session`.

## Design

### The interface

`crates/iris-session/src/isolation.rs` keeps the trait of the launcher
spec, with two changes:

- `system(data_dir: &Path)` takes the data folder. The isolation keeps one
  record for each session in `<data dir>/sessions/`, and it hides the data
  folder in each view (see [Files](#files)). The engine and the helper make the isolation
  with the same data folder: the engine gives the helper its resolved
  data folder as the first argument (`iris session-hold <data dir> ...`,
  launcher spec, "The hold helper"), never in the arguments or the
  environment of the agent.
- A new method `spawn(&self, session, command: &mut process::Command) ->
  io::Result<Child>` starts the command that `command()` gave. The hold
  helper calls it in place of `Command::spawn`. The default method calls
  `Command::spawn`, so the test isolation does not change. The Linux
  isolation does its setup work here, in the helper, before the agent runs
  (see [Linux](#linux)).

`spawn` runs only after the helper read `start` (launcher spec, "The hold
helper"). It returns when the agent can run, in at most 1.5 seconds, so
that the helper writes `started <pgid>` or `failed` inside the 2-second
start gate of the launcher. A sandbox that is slower fails the start
(`failed`), and the launch ends with `launch_failed` (fail closed). If `spawn` fails after a
process started, it kills that process and reaps it before it returns the
error, so the helper writes `failed` and no process of the session stays.
No code of the agent runs before `spawn` returns `Ok`.

### Linux

The Linux isolation uses bubblewrap (`bwrap`), the sandbox program of
Flatpak. It is a system package (`bubblewrap`). The isolation looks for
`bwrap` at `/usr/bin/bwrap`, then in the absolute folders of `PATH`.

`command()` gives `bwrap` with these options, then `--` and the program
and arguments of the adapter:

- `--unshare-all`: new user, mount, PID, network, IPC, UTS and cgroup
  namespaces. The network namespace has only the loopback interface.
- `--die-with-parent`, `--new-session`.
- `--info-fd 3` and `--block-fd 4`: `bwrap` writes the ID of its first
  process (the "init" of the PID namespace) to file descriptor 3, then
  waits for one byte on file descriptor 4 before it runs the program.
- The file view of [Files](#files).
- `--chdir <workspace>`.

`spawn()` in the helper does these steps:

1. Make two pipes, and start `bwrap` with their ends on file descriptors 3
   and 4. `bwrap` sets up the namespaces and waits.
2. Read the JSON line of file descriptor 3 (at most 4 KiB). Steps 2 to 5
   share one time limit of 1.5 seconds.
   It gives `child-pid`: the init, as a host process ID. The isolation
   reads no other field.
3. Get the identity of the init: open a `pidfd` of the process ID, read
   the start time of the process (field 22 of `/proc/<pid>/stat`, in clock
   ticks after the boot) and the inode of `/proc/<pid>/ns/pid`, then check
   that the `pidfd` is not readable (the init did not exit). The init is
   blocked on file descriptor 4, so it cannot exit by itself. A process ID
   stays with its process until the reap, so if the init still runs after
   the reads, the values are the ones of the init. Else `spawn` fails.
4. Publish the record `<data dir>/sessions/se-<id>.sandbox`: the boot ID
   (`/proc/sys/kernel/random/boot_id`), the init process ID, its start
   time and the namespace inode. Write the line to the temporary file
   `se-<id>.sandbox.tmp` (mode `0600`, truncated: only the helper of this
   session writes it), `fsync` it, then link it to the record name
   (`link`, which fails if the record exists), `fsync` the folder and
   remove the temporary file. So a record is complete or absent, also
   after a crash. A crash before the link leaves no record: the init waits
   on file descriptor 4 and ends with the helper (`--die-with-parent`), so
   the program of the adapter did not run, and `ended` is true. A crash
   can leave the temporary file: it holds four numbers and no secret.
5. If the session has an egress port: make the listener of the session in
   its network namespace (see [Egress on Linux](#egress-on-linux)).
6. Write one byte to file descriptor 4. `bwrap` runs the program of the
   adapter.

If a step fails, `spawn` sends `SIGKILL` to the init and to `bwrap`, reaps
`bwrap` and returns the error. The program of the adapter did not run.

#### Lifetime on Linux

The init of the PID namespace is a child of `bwrap`, and `bwrap` is the
child of the helper. With `--die-with-parent`, each of the two gets
`SIGKILL` from the kernel when its parent ends (`PR_SET_PDEATHSIG`). When
the init of a PID namespace ends, the kernel kills each process in the
namespace, also a process that called `setsid`, and the init becomes a
zombie only after each of them ended. So:

- The helper ends for any reason, also `SIGKILL`: `bwrap` ends, the init
  ends, and each process of the session ends. No call from the engine is
  necessary.
- The helper sends `SIGTERM`, then `SIGKILL`, to the process group of
  `bwrap` (the agent group of the launcher spec): `bwrap` ends, and the
  same chain ends each process of the session.

The identity of the init is the boot ID, the process ID and the start
time. Two processes of one boot cannot have the same process ID and the
same start time, because an ID goes to a new process only after the reap
of the old one, and the start time of the new process is then the same or
later; the namespace inode is a fourth check. The identity check: open a
`pidfd` of the process ID, read the start time and the namespace inode,
then check that the `pidfd` is not readable, so the reads were of the
process of the `pidfd`.

- The boot ID differs: the system started again, so each process of the
  record ended.
- No process has the ID, or the process has another start time or another
  namespace inode: the init of the record ended (its ID went to another
  process). Nothing gets a signal.
- The four values agree: this is the init of the record.

A tie is possible in one case only: the init ended, and in the same clock
tick (10 ms) a new process got the same process ID in a PID namespace
with the same inode. The process ID of Linux goes up to `pid_max` before
it wraps, so this case needs a full wrap of the process IDs inside one
tick. The design takes this risk.

`stop(session)` reads the record. If there is no record, `bwrap` did not
reach step 4, and its init cannot outlive `bwrap`: `stop` returns `Ok`.
Else it runs the identity check. Only if the four values agree, it sends
`SIGKILL` through the `pidfd` and waits at most 5 seconds until the
`pidfd` is readable (the init is a zombie, so the namespace is empty).

`ended(session)` is true if there is no record, if the identity check says
that the init of the record ended, or if the init is a zombie. Each other
result is false, and an error (for example a record that cannot be read)
is an error: a false "still runs", never a false "ended".

`name(session)` is `bwrap:<path of the record>`.

#### Egress on Linux

The network namespace of the session has no route to the host. The engine
listens on the egress port of the session on the host loopback address
(launcher spec). The agent connects to `127.0.0.1:<port>` in its own
namespace, so a second listener and a relay connect the two:

1. In step 5 of `spawn`, the helper forks one child process. The child
   opens `/proc/<init>/ns/net`, gets the user namespace that owns it
   (`ioctl NS_GET_USERNS`), and joins the user namespace, then the network
   namespace (`setns`). The helper is the owner of that user namespace, so
   the kernel permits the join. The child binds a TCP listener on
   `127.0.0.1:<port>` in the namespace of the session, sends its file
   descriptor to the helper (`SCM_RIGHTS`), and exits. The child runs only
   system calls between the fork and its exit.
2. The helper starts one relay thread. It accepts each connection on the
   listener, opens a connection to `127.0.0.1:<port>` on the host, and
   copies the bytes in the two directions. It holds at most 8 connections,
   as the egress proxy does, and closes a ninth at once.
3. The relay threads end with the helper. The proxy of the session keeps
   its port until the reap check confirms that the helper ended (launcher
   spec, "Model egress", changed in this PR). So, while the engine runs,
   the relay never connects to a port that another session got.
4. The relay connects to the host from the session address of the
   session: `127.128.0.0` plus the session ID (`session_peer` in
   `egress.rs`, changed in this PR). Linux takes each address of
   `127.0.0.0/8` as a local address with no setup. The engine starts the
   proxy of the session with this address (`Proxy::start` gets a `peer`
   argument), and the proxy refuses a client from each other address with
   the log word `not_session`. Session IDs are never used again (the
   `AUTOINCREMENT` key of the sessions table), so two sessions never have
   the same address. An engine crash frees the port of each proxy, and a
   new engine can give a port to another session. The relay of an old
   session can then reach the proxy of a new session, but that proxy
   refuses it. This holds with no timing assumption.
5. As a second guard, the relay also stops when the engine that started
   the helper ends: `spawn` opens a `pidfd` of the parent, and the relay
   checks it before each connection. Step 4, not this check, keeps the
   sessions apart.
6. A session ID above `SESSION_MAX` (`0x7FFFFE`) has no session address.
   `spawn` of such a session with an egress port fails with a reason. An
   installation reaches this limit only after about 8 million sessions.

The agent reaches only the listener of its own session. The listener
exists only in the namespace of that session, and the relay connects only
to the egress port of that session, which the launch gave in the
`HTTPS_PROXY` value of `command()`. A session with no egress port gets no
listener: its namespace has no listening socket and no route.

### macOS

This card gives no isolation on macOS. `system()` gives the reason:
"agent sessions need Linux: macOS has no sandbox that can stop each
process of a session for sure (R4-05)". So `iris job run` is refused on
the Mac, and no agent runs there with a weaker sandbox.

The reason: `sandbox-exec` can limit the files and the network of a
process, but macOS gives no public mechanism that finds each process of
one sandbox and stops it with no race:

- No PID namespace and no parent death signal, so the lifetime rule has
  no kernel support.
- A list of the processes (`proc_listallpids`) and a check of each one
  (`sandbox_check`) is not atomic. A process that forks and exits again
  and again can stay out of each list. Then `ended` is true while a child
  runs, the engine reaps the session, and the child can read the mail that
  the next session of the same job gets in the same workspace path.
- `kqueue` with `NOTE_TRACK` (follow each fork) is not supported on
  macOS. Endpoint Security needs an entitlement and root.

The macOS card looks for a sound design, for example a small Linux virtual
machine (macOS 26 has the Containerization framework) that runs this Linux
sandbox. This decision and the card get an issue.

### Files

The agent sees only these paths.

| Path | Access |
|---|---|
| The workspace of the job | Read and write |
| The private home of the session | Read and write |
| System programs and libraries: `/usr` without `/usr/local`, and `/bin`, `/sbin` and `/lib*` (a link to `/usr`, or the folder) | Read |
| The system files that programs read at the start, each one if present: in `/etc`, the files `ld.so.cache`, `ld.so.conf`, `passwd`, `group`, `nsswitch.conf`, `hosts`, `localtime`, `os-release`, and the folders `ld.so.conf.d` and `alternatives`; only the public certificate authority files: `ssl/certs`, `ssl/cert.pem`, `ssl/openssl.cnf`, `pki/tls/certs`, `pki/tls/cert.pem`, `pki/tls/openssl.cnf`, `pki/ca-trust/extracted` and `ca-certificates/extracted`. The folders of private keys (`ssl/private`, `pki/tls/private`) are not in the view | Read |
| The file of the adapter program (its real path) and the file of the `iris` program | Read |
| A new `/proc`, a minimal `/dev` | As `bwrap` makes them |
| The private `/tmp`: an empty `tmpfs` of the session, gone at its end | Read and write |
| The root of the view (a `tmpfs` of `bwrap`), the empty `/usr/local` and the empty data folder (both `tmpfs`), and each other folder that the view makes | Read only: `--remount-ro` of `/usr/local`, of the data folder and of `/` after the last mount. A remount is not recursive, so the workspace keeps its write access |

Each other path is not visible. This holds the data folder, the home
folder of the person, `/opt`, `/srv`, `/var`, `/run`, each other file of
`/etc`, `/usr/local`, and each socket outside the workspace (for example a Docker or SSH
agent socket, which a network namespace does not block). The view shows
no folder of local programs or settings that the system does not own: a
credential that a person or a program puts there stays out. The `/etc`
list holds public system data only (programs, certificate authorities,
names of users and hosts). `/usr` holds the files of the system
packages. `/usr/local` holds local programs and settings, so the view
mounts an empty `tmpfs` there; a program file in it is shown alone, as
each adapter program is.

- The private home: an empty `tmpfs` at the path of `HOME`. It has no
  file of the person, and it is gone when the session ends. So the agent tool of a session authenticates with its key
  variable (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or a `pass_env` of
  `agents.toml`). This card adds `CLAUDE_CODE_OAUTH_TOKEN` to the variables
  of the `claude-code` adapter, so a person with a Claude subscription can
  use a token from `claude setup-token`.
- The isolation shows the program file only, not its folder. A folder
  such as the home of the person, or `~/.local/bin`, can hold credentials
  and other files of the person. So an adapter program that needs other
  files outside the system folders (for example a Node.js program of
  `nvm` in the home folder) does not run in a session. Known limit, with
  an issue: a later card can add a list of read paths to `agents.toml`,
  which the person sets.
- The data folder is always empty in the view: the isolation mounts an
  empty `tmpfs` there after each read path and each program file, and
  only then gives the workspace. So a data folder inside `/usr` (for
  example `/usr/share/iris`) is supported, and the view does not show it.
- A program file inside the data folder (the adapter program or the
  `iris` program) is not in the view, because the empty mount hides it.
  The agent then does not start, and the session ends with the exit
  status of `bwrap`. Nothing of the data folder is read. Known limit, with
  an issue: install the programs outside the data folder.
- The workspace is mounted after the private home, so the view
  also holds when the data folder is inside `HOME`.

### Check

`bwrap` is found, and one probe run of `bwrap` with the options of
`command()` (the data folder in the place of the workspace, so each
mount of a real view happens) and the program `true` exits with 0 in 5
seconds. The kernel
gives a `pidfd` (`pidfd_open` exists, Linux 5.3). A system that refuses
user namespaces (for example an AppArmor rule) fails the probe; the reason
names the `bwrap` error.

## Options not chosen

- Our own namespace code (`unshare`, `pivot_root`, mounts) on Linux: more
  security code to write and to test, and Ubuntu 24.04 lets only
  programs with an AppArmor profile, such as `bwrap`, make a user
  namespace.
- Landlock and seccomp with no network namespace (the Codex Linux sandbox):
  Landlock limits TCP connections by port only, not by address, and
  needs Linux 6.7. An agent could reach a remote host on the port number
  of the egress port.
- `slirp4netns` or `pasta` for the network: more programs to install, and
  they give a full network that a second filter must close.
- A relay program inside the sandbox (for example `socat`, or a hidden
  `iris` verb) that connects to a Unix socket: one more program in the
  sandbox, and one more verb in the engine.
- A cgroup for each session on Linux (`cgroup.kill`): needs cgroup v2
  and a delegated subtree, which a person who starts `iris` from a shell
  does not have.
- A file view that shows the whole disk read-only, minus a deny list
  (Anthropic `sandbox-runtime`): a deny list misses each secret and each
  socket that it does not name.
- On macOS, `sandbox-exec` with a process list (`proc_listallpids` and
  `sandbox_check`), and a watchdog process: the list is not atomic, so a
  process that forks and exits again and again can stay out of it (see
  [macOS](#macos)). The process group and the parent process ID do not
  help either: `setsid` and an exit of the parent remove both.

## Failure list

Supported input: the session ID, the workspace, the egress port and the
adapter that the launch gives; a program that the agent runs inside the
sandbox. Each other condition fails closed: `check` fails, `spawn` fails
before the agent runs, or `ended` is false.

| Input or event | Result | Test |
|---|---|---|
| An adapter program in the home folder of the person (for example `/home/alice/my-tool`) | Only the file is in the view: `/home/alice/.ssh` is not | Linux sandbox test with a program in a folder that also holds a marker file |
| The init exits or its process ID is reused before the namespace read | The `pidfd` check fails: `spawn` fails, no record | Unit test of the identity read on a process that exited |
| `bwrap` is missing, or the system refuses a user namespace | `check` fails with the reason; `run` is refused | Linux test with a missing `bwrap` path |
| The agent connects to an address other than `127.0.0.1:<port>` (a public address, a host port, the port of another session) | No route, or connection refused; nothing is reached | Linux sandbox test |
| The agent sends UDP (for example DNS) | No route | Linux sandbox test |
| The agent connects to `127.0.0.1:<port>` | The relay reaches the egress port | Linux sandbox test with a test listener |
| A session with no egress port | No listener and no route | Linux sandbox test |
| The agent reads the store key, `iris.sock`, another workspace or a file of the home of the person | The path does not exist in the view | Linux sandbox test |
| The agent writes outside the workspace and the private home | Refused (read-only or not present) | Linux sandbox test |
| The agent calls `setsid` and the helper ends with `SIGKILL` | The kernel ends the process | Linux lifetime test |
| The agent calls `setsid`, then the helper stops the group and calls `stop` | `stop` kills the init; `ended` is true | Linux lifetime test |
| The engine restarts while a session runs | `stop` and `ended` read the record | Linux test with a new isolation value |
| The init process ID is reused by another process | The start time or the namespace inode differs: no signal; `ended` is true | Unit test with a record of this test process with another start time, and one with another namespace inode |
| The system started again after the record | The boot ID differs: no signal; `ended` is true | Unit test with a record of another boot ID |
| The record cannot be written | `spawn` fails; the program did not run | Linux test with a read-only sessions folder |
| The info line or the listener does not come in 1.5 seconds, or the line is not JSON | `spawn` fails and kills `bwrap` | Unit test of the parser (property test) |
| A ninth connection to the relay | Closed at once | Linux relay test |
| The engine crashes, and a new engine gives the port of an old session to the proxy of a new session while the helper of the old session still runs | The relay of the old session connects from its own session address, and the new proxy refuses it (`not_session`). Also, the relay makes no new connection after the end of its engine | Linux test: a relay of session 7 to the proxy of session 8 gets 403 and the log word `not_session`. Proxy test: only the session address passes. Relay test: the parent of the relay ends, and a new listener on the same port gets no connection |
| A host process that is not Iris listens on the freed port after an engine crash | The relay can connect to it until the helper stops the agent (the end of the pipe from the engine), and the relay stops at the end of its engine. The agent tool verifies TLS to the model host, so that process cannot read the tunnel | Known limit (issue) |
| A file view of a host with keys in `/etc/ssl/private` | That folder is not in the view | Linux sandbox test: `/etc/ssl` in the view holds no `private` |
| A session ends, and its agent connects to the relay during the stop | The relay reaches the proxy of its own session. That proxy is closed at `ended_at`: it ends each open tunnel and answers each new connection with a close (`ended reason=stopped`), and it keeps its port until the reap check | Proxy test: after `close`, an open tunnel ends, a new connection gets `stopped`, and the port stays bound. R4-04e supervisor test: a second session starts while the first one stops, and gets another port |
| A program file inside the data folder (the adapter program or the `iris` program) | The empty mount of the data folder comes after each program file, so it hides the program. The agent does not start, and the data folder stays empty in the view. Known limit (issue) | Linux sandbox test with the program file inside the data folder: the program does not run |
| A credential file or a Unix socket in a folder that is not in the view (for example `/etc/<file>` not in the list, or `/run`) | Not visible: no read and no connection | Linux sandbox test: a socket and a marker file outside the view, and `/etc/hostname` |
| `iris job run` on macOS or another system | `system()` gives the reason, and `run` is refused before a row is written | Unit test that runs on each system that is not Linux |

## Impact

- `crates/iris-session/src/isolation.rs`: `system(data_dir)`, the trait
  method `spawn`, and the selection of the system.
- New files beside it: `isolation_linux.rs` and its tests.
- `crates/iris-session/src/hold.rs`: the helper calls `Isolation::spawn`.
- `crates/iris-session/src/egress.rs`: `session_peer`, `connect_from`,
  the `peer` argument of `Proxy::start` and the refusal `not_session`. The
  engine (R4-04e) passes `session_peer(session)` when it starts a proxy.
- The launcher spec, "Model egress": the proxy takes only the session
  address of its session.
- `crates/iris-session/src/config.rs`: `CLAUDE_CODE_OAUTH_TOKEN` in the
  `claude-code` adapter, also in the adapter rule of the launcher spec.
- The launcher spec, "Model egress": at `ended_at` the engine closes the
  proxy of the session (`Proxy::close`, new in `egress.rs`: no tunnel and
  no new connection), and the proxy keeps its port until the reap check
  passes. R4-04e calls `close` and `stop`.
- The launcher spec, "The hold helper": the helper runs outside the
  sandbox, and the agent (`bwrap` on Linux) is its child.
- `docs/spec/03-job-workspace.md` and the fixed text of `JOB.md` in
  `crates/iris-workspace`: the session has no network other than the hosts
  that `agents.toml` allows for the agent tool (its model host), and the
  agent tool can send the folder to those hosts before `iris submit`
  (launcher spec, "Model egress").
- `starter.json`: `bubblewrap` in the system packages of the `engine` area,
  for the tests.
- The engine (R4-04e) calls `system(data_dir)` in the engine and in the
  helper.

Memory: the Linux relay uses one accept thread, and two threads with one
16 KiB buffer each for each open connection, at most 8 connections: about
300 KiB at most in the helper. The private home on Linux is a `tmpfs`, so
the files of the agent in it count as memory until the session ends.

## Assumptions

- An unprivileged process can join the user namespace that `bwrap` made,
  then its network namespace, and bind a listener there that the sandbox
  sees. Evidence: a probe as the user `nobody` on Linux 6.18 with
  `bwrap` 0.9.0 (six runs, each one passed; the sandbox reached the
  listener, and `1.1.1.1` gave "Network is unreachable").
- `bwrap --info-fd` writes a JSON object with `child-pid`. Evidence:
  `bwrap(1)`, and the probe output of `bwrap` 0.9.0:
  `{"child-pid": 2355, "cgroup-namespace": ..., "pid-namespace": ...}`.
  The design reads only `child-pid`, so an older `bwrap` with fewer fields
  works too.
- The user namespace of `/proc/<init>/ns/user` can change after the info
  line (`bwrap` makes a second user namespace), so the join uses the owner
  of the network namespace (`NS_GET_USERNS`). Evidence: the same probe
  failed one time in four with `EPERM` when it joined `/proc/<init>/ns/user`.
- `--die-with-parent` gives `PR_SET_PDEATHSIG` to `bwrap` and to the init.
  Evidence: `bwrap(1)`. The Linux lifetime test proves it.
- The init of a PID namespace becomes a zombie only after each process of
  the namespace ended. Evidence: `pid_namespaces(7)`. The Linux lifetime
  test proves it.
- `bwrap` of Ubuntu 24.04 can make a user namespace under the AppArmor rule
  of that release. Evidence: pending. If not, `check` fails with the reason
  and no agent runs (fail closed).
- `claude -p` runs with an empty home and `ANTHROPIC_API_KEY` or
  `CLAUDE_CODE_OAUTH_TOKEN`. Evidence: pending, the check of the
  launcher spec (issue #404). If not, the session ends with the error of
  the tool, and no mail leaves.

## External writes

None. The isolation writes only files in the data folder and sends
signals to processes of the session.

## Open questions

1. Does `codex exec --sandbox workspace-write` run inside the sandbox?
   Codex makes its own sandbox, and a nested sandbox can fail. If not, the
   `codex` adapter changes its flag in its own PR.
2. How much memory does the helper use with the relay (measure on Linux)?

## Acceptance criteria

1. On Linux with `bwrap`, a session reaches its egress port and no other
   address. Red if the sandbox test reaches a test listener on another
   host port or another address.
2. A session cannot read a file of the data folder outside its workspace.
   Red if the test reads the content of a marker file of the data folder.
3. A process that called `setsid` ends when the helper ends with
   `SIGKILL`. Red if the test finds the process alive 5 seconds later.
4. `ended` is false while a process of the session runs, and true after
   `stop`. Red on any other answer.
5. With no `bwrap`, `check` fails and names `bubblewrap`.
6. On macOS, `system()` gives the reason. Red if it gives an isolation.

## Verify

```bash
cargo test -p iris-session --lib isolation
scripts/local-ci/run.sh
```


## Rollout

None. `iris job run` starts to work when R4-04e and this card are merged.
A Linux server needs the package `bubblewrap`; `docs/DEPLOYMENT.md` says so.

## Risks

- A sandbox escape in `bwrap` or in the kernel. Both are maintained by
  their owners. Rollback: make `system()` return the error again; no
  agent runs.
- The view can miss a file that an agent tool needs. The tool then fails,
  and no data leaves. The check of issue #404 finds such a file.

## Plan

1. R4-05 spec: this file.
2. R4-05 Linux: the interface change, the Linux isolation, the relay and
   the tests.
3. A new card: the design of a macOS sandbox (issue).

## Spec checks

- [x] Each assumption holds with a second caller, worker or tab: each
  session has its own record, namespace and listener.
- [x] Each assumption holds when an outcome is unknown: a crash between
  the record and the start leaves a record of an init that dies with
  `bwrap`; `ended` reads the process, not the record alone.
- [x] Each dropped mechanism was dropped for a reason (Options not chosen).
- [x] No step is check-then-act: the signal goes through a `pidfd` after
  the namespace check, so a reused process ID is never signaled.
- [x] Each property has a test.
- [x] The spec addresses the hardest problem: the lifetime rule and the one
  network path.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result.
