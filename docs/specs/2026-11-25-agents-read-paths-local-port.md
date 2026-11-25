# Spec: read paths and a local model port in agents.toml

- Date: 2026-11-25
- Card: none (open-issues sweep of 2026-11-25, session group: issues #505 and #406)
- Status: Built

## Problem

The sandbox of a session shows only the workspace, the private home, the
system folders and the program files (sandbox spec, "Files"). The egress
proxy connects only to public addresses (launcher spec, "Model egress").
So two kinds of adapter cannot run in a session:

- An adapter program that needs files outside the system folders, for
  example a Node.js program of `nvm` in the home folder, or an npm global
  in `/usr/local` (issue #505).
- An adapter that uses a model server on the machine, for example an
  OpenAI-compatible server on `127.0.0.1:11434` (issue #406). The owner
  runs such a server on the Mac.

## Goal

1. An adapter of `agents.toml` can name folders and files that its
   session can read (`read_paths`). The session cannot write them.
2. An adapter of `agents.toml` can name one TCP port on the loopback
   address of the machine (`allow_local`). A process of its session can
   connect to that port and to no other local port.
3. Both keys are opt-in. With neither key, a session is the same as now.
4. Each rule of the sandbox that holds now still holds: the data folder is
   empty in the view, no other path is visible, no other network.

## Not in scope

- Login files of the built-in adapters (issue #509). The built-in adapters
  `claude-code` and `codex` get no `read_paths`. A login folder holds
  settings (for example hooks) and tokens that the tool writes at each
  refresh. A read-only view breaks the refresh, and a writable copy can
  use the refresh token of the person and break the login outside the
  session. The key variables stay the only login of a session.
- Write access to a read path.
- More than one local port for each adapter, a local address other than
  `127.0.0.1`, and UDP.
- A port on another machine of the local network.

## Design

### The keys

```toml
[agent.local-model]
command = ["/Users/me/.nvm/versions/node/v22.11.0/bin/node", "/Users/me/tools/agent.js"]
read_paths = ["/Users/me/.nvm/versions/node/v22.11.0", "/Users/me/tools"]
allow_local = 11434
```

- `read_paths` has 0 to 16 strings. Each one is an absolute path. The
  parse refuses a relative path, an empty string, a path with a `..`
  part, and a string with a NUL byte.
- `allow_local` is a TCP port from 1 to 65535. The session can connect to
  `127.0.0.1:<port>` of the machine.
- The keys are only for a name of the file. `claude-code` and `codex`
  stay names that the file cannot use.

### The checks at each run

The engine reads `agents.toml` at each `run` (launcher spec, "Adapters").
After the parse, the launch resolves each read path (`canonicalize`) and
checks it.

**Invariant.** No read path shows an object of a protected folder: the
data folder, a home folder as a whole, or a folder of system state.

**Who the checks hold against.** The checks hold against a wrong or
careless `agents.toml` and against the agent of a session, which cannot
rename or mount a host file. A process of the person on the host has the
rights of the engine: it can read the data folder directly, so a race by
such a process (for example a rename exchange of a read path and a
file of the data folder between two checks) gives it nothing new and is
out of scope. The helper still binds the opened object, and compares its
current path (`/proc/self/fd/<n>`) with the checked real path, so a
change of the path between the checks stops the start.

**The home folders.** `HOME` can be absent or wrong (a service, or a data
folder that `IRIS_DATA` or `--data` sets). So the protected home folders
are each of these that exists: the real path of `HOME` of the engine, the
home folder of the user of the engine in the user database
(`getpwuid(geteuid())`), and the home folder of each other account of the
user database (`getpwent`). If the user database gives no home folder for
the user of the engine, the run of an adapter with read paths is refused. A path is only one name of a folder:
a bind mount gives the same folder a second name, also with the mount
root `/`. So the checks compare the place of a folder in its file system,
not its name in the mount tree.

**The place of a folder** (Linux) is the pair (file system, path in that
file system). The launch reads `/proc/self/mountinfo` and finds the mount
of the real path (the longest mount point that holds it; for the same
point, the last line). The place is the device (`major:minor`) of that
mount, and the root of the mount joined with the rest of the real path
below the mount point. A bind mount of a folder at another name has the
same place as the folder. On macOS there are no bind mounts, so the place
is the real path.

The launch refuses the run, with the path and the reason, if:

- the path does not exist or cannot be resolved, or `mountinfo` cannot be
  read or parsed (fail closed);
- one of these is true, compared by place on the same device:
  - the read path holds the place of the data folder, is inside it, or is
    the same place;
  - the read path holds the place of a home folder, or is the same
    place. A folder inside a home folder is allowed: `~/.nvm` passes,
    `/home`, `/Users` and `/home/alice` do not;
  - the read path holds, is inside, or is a folder of system state:
    `/proc`, `/sys`, `/dev`, `/run`, `/var/run`, `/tmp`, `/var/tmp`,
    `/private/var/run`, `/private/tmp`, `/private/var/folders`,
    `/Volumes` and `/System/Volumes/Data`. These folders hold sockets,
    devices, process data and the files of other programs. This check
    also compares the real path, so a virtual file system (`proc`,
    `sysfs`, `devtmpfs`) is refused by its type in `mountinfo` too;
- the tree of the path holds an entry that is not a regular file, a
  folder or a link: a socket, a FIFO or a device. A socket is reachable
  through a read-only bind, because a network namespace does not block a
  Unix socket. The walk does not follow links, and it stops with a
  refusal after 200,000 entries;
- the tree of the path holds a regular file with more than one hard link
  on the device of the data folder or of a home folder. A hard link shows
  a file of a protected folder under a second name, and the place checks
  compare names. A hard link stays on one device, so a file on another
  device passes. A tool folder that a package manager filled with hard
  links (for example the store of `pnpm`) on that device is refused;
- the tree of the path holds a mount point. `bwrap` binds a tree with its
  mounts, and a nested mount has a place of its own that the checks above
  do not compare. The walk compares the mount ID of each entry (`statx`
  with `STATX_MNT_ID`, Linux 5.8 or later) with the mount ID of the read
  path. If the kernel gives no mount ID, the run is refused. On macOS the
  walk compares the device number (`st_dev`).

So each folder that the view shows through a read path is the read path's
own place in one file system, with no nested mount, and that place does
not touch a protected place. This holds for each name of a folder,
because the compare does not use the name.

On macOS, Seatbelt also checks the real path of each access, and the
profile denies the data folder after the read paths (see "macOS").

A link inside a read path that points outside it stays a link in the
view. Its target is visible only if the view holds the target.

The launch refuses `allow_local` when it is the same port as the egress
port of the session: the start stops with a reason. The egress port comes
from the system, so this is rare, and the next run of the job gets a new
egress port.

### Linux

- The launch checks happen in the engine. Between the checks and the
  mount, a process of the person can change a part of the path into a
  link. So the hold helper opens each read path again, just before it
  starts `bwrap`, with `openat2` and `RESOLVE_NO_SYMLINKS` (`O_PATH`):
  the open fails if a part of the path is a link. The helper then runs
  each check of the launch again on the object of the descriptor (its
  place, the protected folders and the walk), and gives the descriptor to
  `bwrap` with `--ro-bind-fd <fd> <real>`. The invariant needs no compare
  with the object of the launch: an object that passes each check shows
  no protected folder. If another folder takes the path, the session
  sees that folder only if it passes each check, as a change of
  `agents.toml` would. `bwrap` mounts the object of the descriptor, not a path. If
  an open or a check fails, no session starts. `bwrap` has
  `--ro-bind-fd` since version 0.5.0. An older `bwrap` refuses the
  unknown option and exits, so no session starts (fail closed).
- The binds come after the system folders, the private home and the
  program files, and before the empty mount of the data folder. So a read
  path that holds a part of the data folder cannot show it (the check
  above refuses it first; the mount order is the second guard).
- `allow_local`: the helper binds a second listener in the network
  namespace of the session, on `127.0.0.1:<allow_local>`, in the same
  child process as the egress listener (sandbox spec, "Egress on Linux",
  step 1). A second relay thread accepts each connection and connects to
  `127.0.0.1:<allow_local>` on the host, with the limit of 8 connections.
  The helper reads the port from the pair `--setenv IRIS_LOCAL_PORT <port>`
  in the options of `bwrap` (before `--`), which `command()` writes. The
  agent gets the same variable.
  The relay copies bytes only. The local relay connects from the address
  `127.0.0.1`, not the session address: the server on the port is a
  program of the person, not an Iris proxy.
- No other listener and no route are added.

### macOS

- Each read path is a `(subpath "<real>")` in `file-read*` of the
  Seatbelt profile, and its parent folders are literals in
  `file-read-metadata`. Seatbelt checks the real path of each access at
  the time of the access, so a link that a process makes later points to
  a path that the profile checks again.
- The profile has no `network-outbound` rule for a Unix socket other than
  the session socket, so a socket in a read path is not reachable. The rule comes before the last rule that denies
  `/System/Volumes/Data`, so a read path under that folder is refused
  (Seatbelt takes the last rule that matches).
- The data folder gets a `(deny file-read* (subpath "<data dir>"))` after
  the read paths, as the second guard.
- `allow_local`: one more `(remote tcp "localhost:<allow_local>")` in
  `network-outbound`. TCP only.

### Effect on the agent

A session with `allow_local` gets one new variable, `IRIS_LOCAL_PORT`,
with the port. A session without the key gets no new variable. The
person sets the address
of the model server in the arguments or the settings of the tool, for
example `http://127.0.0.1:11434/v1`. The local relay has no proxy step,
so a client must not send the request to `HTTPS_PROXY`; a tool that does
fails at its first call.

## Options not chosen

- A local host through the egress proxy (`CONNECT 127.0.0.1:<port>`): a
  local model server speaks plain HTTP, and many clients skip the proxy
  for `localhost`. A plain relay is simpler and has no parse.
- `read_paths` for the built-in adapters: see "Not in scope".
- Write access to a read path: the session could then change a program of
  the person outside the session.

## Failure list

| Input | Result | Test |
|---|---|---|
| A read path that is relative, empty, has `..` or NUL | The parse refuses the file, with the line | config test |
| More than 16 read paths | The parse refuses the file, with the line | config test |
| A read path that does not exist at the run | The run is refused, with the path | launch test |
| A read path that is `/`, the home folder, the data folder, inside it or holds it (also through a link) | The run is refused, with the path | launch test |
| A bind mount of the data folder, of a part of it or of the home folder at another name, with any mount root (also `/`) | The place compare refuses it, with the path | unit test of the place from `mountinfo` lines |
| `mountinfo` that cannot be read or parsed, or a mount point with an escape that the parse does not know | The run is refused | unit test |
| A read path that is `/proc`, `/sys`, `/dev`, `/run`, `/tmp` or another folder of system state, inside one, or holds one, also by another name | The run is refused, with the path | launch test |
| A read path that holds `HOME` (for example `/home`) | The run is refused, with the path | launch test |
| `HOME` absent or wrong, and a read path that is or holds the home folder of the user database (for example `/home/alice`) | The run is refused, with the path | launch test with `HOME` unset |
| The user database gives no home folder for the user of the engine | The run of an adapter with read paths is refused | unit test |
| A read path with a socket, a FIFO or a device in its tree | The run is refused, with the path of the entry | launch test |
| A read path with a hard link to a file of the data folder | The run is refused, with the path of the entry | unit test |
| A read path with a mount point in its tree, also a bind mount on the same device | The run is refused, with the path of the entry | launch test (Linux: a bind mount in a private mount namespace of the test, if the test can make one; else a unit test of the mount ID compare) |
| A kernel that gives no mount ID (`STATX_MNT_ID`) | The run of an adapter with read paths is refused | unit test |
| A tree of more than 200,000 entries | The run is refused | launch test |
| A part of a read path that changes into a link after the launch checks | Linux: the open of the helper fails, and no session starts. macOS: Seatbelt checks the real path at each access | Linux helper test with a swap between the check and the open |
| A read path that changes to another folder after the launch checks | The helper checks the new folder: a protected or unsafe folder stops the start; a clean folder is bound | Linux helper test with a swap to the data folder |
| `bwrap` older than 0.5.0 and an adapter with read paths | `bwrap` refuses the option and exits; no session starts | not testable in CI (the CI has 0.9) |
| A write to a read path | The system refuses it (read-only bind) | Linux sandbox test |
| A connection to a local port other than `allow_local` | Refused: no listener in the namespace (Linux), the profile denies it (macOS) | Linux sandbox test |
| `allow_local` with no server on the port | The relay closes the connection | Linux sandbox test |
| `allow_local` 0 or above 65535, or not an integer | The parse refuses the file, with the line | config test |
| A ninth local connection | The relay closes it at once | Linux sandbox test |

All other input: the run is refused, and no session starts.

## Impact

- `crates/iris-session/src/config.rs`: the two keys, their checks.
- `crates/iris-session/src/launch.rs`: the run checks of the read paths.
- `crates/iris-session/src/isolation.rs`: `Paths` (or the `command()`
  input) gets the read paths and the local port.
- `crates/iris-session/src/isolation_linux.rs`: the binds, the second
  listener and relay.
- `crates/iris-session/src/isolation_macos.rs`: the profile.
- The rules of `JOB.md` (`crates/iris-workspace/src/lib.rs`, `RULES`, and
  `docs/spec/03-job-workspace.md`, sections 9 and the sample). The
  `allow_local` PR changes the two lines of the network in both files in
  the same PR, because a test compares them byte for byte:
  - "Nothing leaves this folder before `iris submit`, other than what your
    agent tool sends to the hosts and the local port that `agents.toml`
    allows for it (its model host)."
  - "This session has no network other than the hosts and the local port
    that `agents.toml` allows for your agent tool (its model host), and no
    mailbox credentials."
  The text is the same for each session, so the build of the workspace
  does not need to know the adapter. A session with no `allow_local` has
  no local port, and the line stays true.
- Docs: `docs/AGENT-SESSIONS.md` or the place where `agents.toml` is
  documented for the person.
- The launcher and sandbox specs get a line that points here.

## Assumptions

- `bwrap --ro-bind` of a path after `--tmpfs` of a parent folder works in
  the order of the arguments. Evidence: the program files are bound this
  way now (`isolation_linux.rs`, `system()`).
- A server that listens on `127.0.0.1` accepts a connection from the host
  loopback. Evidence: the egress relay connects to the engine this way.
- Seatbelt `(remote tcp "localhost:<port>")` allows a TCP connection to
  `127.0.0.1:<port>`. Evidence: the egress rule of the macOS profile.
- The macOS part is checked only on the Mac of the owner. Status:
  pending. If it is wrong, the session fails closed (the agent cannot
  read the path or reach the port), and nothing leaves.

## External writes

The read paths are read only. They make no write.

The local relay can carry a write: the agent can send a request that
changes the state of the program on the port (for example a Docker API).
Iris does not read or change the bytes. The person chooses the port, and
the docs say that each request of the agent reaches that program.

| Case | What happens |
|---|---|
| Success | The program answers. The relay copies the answer to the agent. |
| Definite rejection (the program refuses, or no program listens) | The relay copies the answer, or closes the connection of the agent when the connect fails. |
| Unknown result (a time-out, a lost answer, an end of the session or of the engine) | The relay closes the connection of the agent. Iris does not know if the program acted, and records nothing. |
| Retry | Iris never connects again for the agent and never sends bytes again. A retry is a new connection of the agent tool. The agent tool decides it. |


## Open questions

- The name of the docs file for `agents.toml`.

## Acceptance criteria

- A session of an adapter with `read_paths` reads a file in a read path
  and cannot write it. Red if the bind is missing or writable.
- A read path that resolves into the data folder refuses the run. Red if
  the check is missing.
- A session with `allow_local = <p>` reaches a test server on
  `127.0.0.1:<p>`, and cannot reach a second test server on another
  local port. Red if the relay is missing or if a route opens.
- A session with neither key has the same view and network as now. Red
  if a test of the sandbox spec fails.

## Verify

```bash
cargo test -p iris-session
scripts/local-ci/run.sh
```

On the Mac (owner): `iris job session` with an adapter that reads a path
and uses a local model server.

## Rollout

None. Both keys are opt-in.

## Risks

- A program of the person can make a socket in a read path after the
  launch checks. Linux: the bind shows it, and a process of the session
  can connect to it. Known limit, issue #996: the docs tell the person
  to list only folders of programs.
- A person can list a folder with credentials. The session can then send
  them to an allowed host. The docs say so next to the key. The checks
  refuse only the root, the home folder and the data folder.
- A person can name a local port of another service (for example a
  Docker API). The docs say so next to the key.
- Rollback: remove the keys from `agents.toml`; the code revert is one
  PR.

## Plan

1. This spec.
2. `read_paths` (#505): config, launch check, Linux, macOS, docs.
3. `allow_local` (#406): config, Linux relay, macOS profile, docs.
