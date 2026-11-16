# Spec: the engine socket and the command verbs

- Date: 2026-11-16
- Card: R2-15
- Status: Draft

## Problem

Release 2 has changesets, the policy and the ledger, but the `iris` command
has no verb that adds an action. Specification 2, section 8.1, says that the
command sends each such request to the engine through a Unix domain socket,
`iris.sock` in the data folder, and that the command never reads the store
for these verbs. No socket exists. So an agent with a shell cannot propose a
change, and the person cannot withdraw a changeset from the terminal (issue
#219).

## Goal

- `iris serve` listens on `iris.sock` in the data folder, mode `0600`.
- These verbs of specification 2, section 8.2, work through the socket, in
  host mode, with the author `local`:
  - `iris close`, `defer`, `decline`, `correct` `<id>` and `iris archive`,
    `inbox`, `label`, `unlabel`, `read`, `unread`, `trash` `<ids>`, each with
    an optional `--changeset <cs-id>`.
  - `iris changeset <cs-id>`, `iris submit <cs-id> --why <text>` and
    `iris withdraw <cs-id>`.
- The exit codes follow specification 2, section 8.4. Code 4 means that the
  engine is not reachable.

## Not in scope

- Session mode and `.iris/socket` (release 4).
- `iris ledger` and the verbs of release 3.
- The read verbs of release 1 (`status`, `search`, `show`) stay on the
  store, as section 8.1 allows.
- A wait in `submit` for the apply. `submit` prints the state of each
  action at the end of the submit.
- The rules of the policy on a `local` action (see "The work").

## Design

### The socket

1. At the start, `iris serve` checks `<data>/iris.sock`:
   - If a connect to it works, another engine runs. The start fails with
     "another engine runs on this data folder".
   - If the file exists and a connect fails, the file is left from a stopped
     engine. The engine removes it.
2. The engine binds the socket, then sets the mode of the file to `0600`.
   The data folder has the mode `0700` (specification 1, section 5.1), so no
   other OS user can reach the file between the bind and the mode change.
3. At a clean stop (Ctrl-C), the engine removes the file. After a crash, the
   next start removes it (step 1).
4. The engine reads the user ID of the peer of each connection (`SO_PEERCRED`
   on Linux, `getpeereid` on macOS, through `tokio::net::UnixStream::peer_cred`).
   If it is not the user ID of the engine, the engine closes the connection
   with no answer.

### The protocol

One request and one answer on each connection. Each is one line of JSON,
with a newline at the end.

Request:

```json
{"v": 1, "verb": "add", "args": {...}}
```

| `verb` | `args` | Does |
|---|---|---|
| `add` | `{"verb", "targets": ["c-7"], "args": {...}, "why"?, "changeset"?}` | Adds one action. With no `changeset`, it makes a new draft changeset of the author `local`. Answer: `{"changeset": "cs-12", "n": 1}`. |
| `show` | `{"changeset"}` | Answer: the changeset document (specification 2, section 5.1). |
| `submit` | `{"changeset", "why"}` | Submits the draft revision. Answer: the changeset document after the submit. |
| `withdraw` | `{"changeset"}` | Withdraws a changeset of the same author (section 5.5). Answer: the changeset document after the withdraw. |

Answer:

```json
{"ok": true, "result": {...}}
{"ok": false, "code": 2, "error": "the reason"}
```

`code` is the exit code of section 8.4 that the command uses. The engine
gives 2 for a request that is not valid (an unknown verb, a target that is not
an ID, a changeset of another author), 1 for an error in the store, and 3
when the policy or a hard limit denied an action at the submit. For `submit`,
the command computes the code from the states of the actions in the
document: 1 if one is `failed`, 3 if one is `denied`, 5 if one is open, else
0 (the order of section 8.4).

### Limits

- A request line is at most 64 KiB. The engine reads at most that, and
  answers an error with code 2 for a longer line.
- The engine waits at most 10 s for the request line, and at most 64
  connections run at the same time. Each connection is one task on the one
  runtime thread of the engine, with one buffer of at most 64 KiB.
- The command waits at most 30 s for the answer.

### The work

Each request is one call of the `iris-changeset` functions: `create` with
`add_action`, `add_action`, `write_document`, `submit` or `withdraw`. Each call
is one SQLite transaction. The engine runs it on a blocking thread
(`spawn_blocking`), so the runtime thread does not wait on SQLite.

After `submit`, each valid action is `proposed`. Today only `iris-triage`
runs the policy on its own actions after its submit (section 5.4, step 5).
This card does not run the rules on a `local` action: the action stays
`proposed`, so the review of the web client lists it, and only the person
approves or denies it. This is the safe side of the policy: a rule can only
approve or deny with no person, and here the person always decides. A hard
limit that denies (section 6.5) is also a decision that the person makes in
the review for now. The general policy step for each author is a known limit
(issue #252), to build with the next author that needs rules.

### The command

The command finds the data folder as today (`--data`, `IRIS_DATA`, or the
folder of the system), connects to `iris.sock`, sends the request and prints
the answer:

- `add`: `cs-12 1` (the changeset ID and the action number).
- `changeset`, `submit`, `withdraw`: one line for each action, which starts
  with `<revision>.<n>`, then the verb, the state and the reason. `--json`
  prints the document.

## Options not chosen

- HTTP on the TCP port of the web client with a token file: the socket gives
  the user ID of the caller from the OS, and needs no secret on disk.
  Specification 2 already chose the socket.
- HTTP over the Unix socket (axum on a `UnixListener`): it works, but one JSON
  line is smaller to parse, to test and to bound. The engine already has
  `serde_json`. A library for a JSON-RPC protocol is not necessary for four
  verbs.
- A socket with no check of the peer user ID: the folder mode is the main
  guard, and the check costs one system call. It stops a socket file that
  other users can reach by mistake (for example, a data folder on a shared
  disk with another mode).

## Failure list

| Input or event | Result | Test |
|---|---|---|
| No engine runs (no file, or a stale file) | The command exits 4: "the engine is not reachable: start it with `iris serve`". | A command test with no engine. |
| A stale socket file at the start | The engine removes it and binds. | An engine test with a left file. |
| A second `iris serve` on the same data folder | It fails with "another engine runs". The first engine keeps the socket. | An engine test. |
| A peer of another OS user | The engine closes the connection with no answer. | A unit test of the check function with another user ID. |
| A line that is not JSON, a wrong `v`, an unknown verb, missing or extra fields | An answer with `ok: false`, code 2. Nothing is written. | A property test: random lines give code 2 and write no row. |
| A line over 64 KiB, or no newline | Code 2, then the engine closes. | A test with 64 KiB plus 1 byte. |
| No request in 10 s | The engine closes the connection. | A test with a short limit. |
| More than 64 connections | The engine waits to accept until one ends. | A test with the limit set to 1. |
| The engine stops after the transaction and before the answer | The command gets no answer and exits 1: "the result is not known: run `iris changeset <cs-id>`" (or, for an `add` with no changeset, "run `iris status`"). The draft can hold the action. A draft has no effect before `submit`. | A test that closes the connection before the answer. |
| The engine stops before the transaction ends | SQLite rolls back. Nothing is written. Same message as above. | Covered by the store tests of transactions. |
| Two commands add to the same draft at the same time | SQLite runs the two transactions one after the other. Each gets its own action number. | A test with two threads. |
| A changeset of another author (`agent`, `person`, `triage`) | Code 2, "only the author can do this" (`check_author`). | A test. |

All other input: code 2 and no write.

## Impact

- `crates/iris/src/socket.rs` (new): the listener, the protocol and the work.
- `crates/iris/src/lib.rs`: the new subcommands, the client, and the start of
  the listener in `serve`.
- `docs/spec/02-changeset-policy-ledger.md`, section 8: a pointer to this
  spec.
- `crates/iris/README.md` or the help text of each verb.
- No change to the store, the API or the web client.
- Memory: one task and one buffer of at most 64 KiB for each connection, so
  at most about 4 MiB with 64 connections. No memory when no command runs.

## Assumptions

- `tokio::net::UnixStream::peer_cred` gives the user ID on Linux and macOS.
  Evidence: tokio 1.54 `src/net/unix/stream.rs` and `ucred.rs` (both
  systems are supported).
- The data folder has the mode `0700`. Evidence: specification 1, section
  5.1, and `iris-store` (`Store::open`).
- A socket path is at most about 104 bytes on macOS and 108 on Linux. A long
  data folder path can pass the limit. `pending`: the engine fails at the
  start with the path in the error, and the person sets `--data` to a
  shorter path. Issue to file if this happens.

## External writes

None. The verbs write only the local store. A mailbox change happens only
after the submit, through the policy and the apply of specification 2.

## Open questions

- The exact text of each line of the command output.

## Acceptance criteria

- With `iris serve` running, `iris close c-7` prints `cs-N 1`, and
  `iris changeset cs-N` shows a draft action `close` with the target `c-7`.
  Red if the command reads the store, or if no draft is made.
- `iris submit cs-N --why "Done."` prints the state of each action and exits
  5 while the action waits for the person. Red if it exits 0.
- `iris withdraw cs-N` makes each open action `cancelled`. Red if an applied
  action changes.
- With no engine, each of these verbs exits 4. Red if one opens the store.
- `ls -l <data>/iris.sock` shows `srw-------`. Red with another mode.
