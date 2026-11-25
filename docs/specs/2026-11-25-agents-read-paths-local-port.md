# Spec: read paths and a local model port in agents.toml

- Date: 2026-11-25
- Card: none (open-issues sweep of 2026-11-25, session group: issues #505 and #406)
- Status: Draft

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
refuses the run, with the path and the reason, if:

- the path does not exist or cannot be resolved;
- the real path is `/`;
- the real path is the data folder, is inside it, or holds it;
- the real path is the home folder of the person (the real path of `HOME`
  of the engine), or holds it. A folder inside the home folder is
  allowed. So `/home` or `/Users` is refused, and `~/.nvm` is allowed;
- the real path is, is inside, or holds a folder of system state:
  `/proc`, `/sys`, `/dev`, `/run`, `/var/run`, `/tmp`, `/var/tmp`,
  `/private/var/run`, `/private/tmp`, `/private/var/folders` and
  `/Volumes`. These folders hold sockets, devices, process data and the
  files of other programs;
- the tree of the path holds an entry that is not a regular file, a
  folder or a link: a socket, a FIFO or a device. A socket is reachable
  through a read-only bind, because a network namespace does not block a
  Unix socket. The walk does not follow links, and it stops with a
  refusal after 200,000 entries;
- the tree of the path holds a mount point (an entry on another device
  than the read path itself). A mount can bring a virtual file system,
  such as a `/proc`, into the view.

A link inside a read path that points outside it stays a link in the
view. Its target is visible only if the view holds the target.

The launch refuses `allow_local` when it is the same port as the egress
port of the session. The egress port comes from the system, so the
launch takes a new egress port in that case.

### Linux

- The launch checks happen in the engine. Between the checks and the
  mount, a process of the person can change a part of the path into a
  link. So the hold helper opens each read path again, just before it
  starts `bwrap`, with `openat2` and `RESOLVE_NO_SYMLINKS` (`O_PATH`):
  the open fails if a part of the path is a link. The helper checks that
  the open file is on the same device and inode that the launch checked,
  then gives the file descriptor to `bwrap` with `--ro-bind-fd <fd>
  <real>`. `bwrap` mounts the object of the descriptor, not a path. If
  an open or a check fails, no session starts. `bwrap` has
  `--ro-bind-fd` since version 0.5.0; the check of the isolation refuses
  an older `bwrap` when an adapter has read paths.
- The binds come after the system folders, the private home and the
  program files, and before the empty mount of the data folder. So a read
  path that holds a part of the data folder cannot show it (the check
  above refuses it first; the mount order is the second guard).
- `allow_local`: the helper binds a second listener in the network
  namespace of the session, on `127.0.0.1:<allow_local>`, in the same
  child process as the egress listener (sandbox spec, "Egress on Linux",
  step 1). A second relay thread accepts each connection and connects to
  `127.0.0.1:<allow_local>` on the host, with the limit of 8 connections.
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

The variables of the session do not change. The person sets the address
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
| A read path that is `/proc`, `/sys`, `/dev`, `/run`, `/tmp` or another folder of system state, inside one, or holds one | The run is refused, with the path | launch test |
| A read path that holds `HOME` (for example `/home`) | The run is refused, with the path | launch test |
| A read path with a socket, a FIFO or a device in its tree | The run is refused, with the path of the entry | launch test |
| A read path with a mount point in its tree | The run is refused, with the path of the entry | launch test |
| A tree of more than 200,000 entries | The run is refused | launch test |
| A part of a read path that changes into a link after the launch checks | Linux: the open of the helper fails, and no session starts. macOS: Seatbelt checks the real path at each access | Linux helper test with a swap between the check and the open |
| A read path that changes to another object (other device or inode) after the launch checks | The helper refuses it, and no session starts | Linux helper test |
| `bwrap` older than 0.5.0 and an adapter with read paths | The check refuses the run | unit test of the version parse |
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
