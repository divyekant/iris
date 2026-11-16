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
  host mode, with the author `local` (the syntax is in "The command"):
  - the verbs that add an action: `close`, `defer`, `decline`, `correct`,
    `archive`, `inbox`, `label`, `unlabel`, `read`, `unread` and `trash`;
  - `iris changeset <cs-id>`, `iris changeset --drafts`,
    `iris submit <cs-id> --why <text>` and `iris withdraw <cs-id>`.
- The exit codes follow specification 2, section 8.4. Code 4 means that the
  engine is not reachable.

## Not in scope

- Session mode and `.iris/socket` (release 4).
- `iris ledger` and the verbs of release 3.
- The read verbs (`status`, `search`, `show`, `brief`, `list`) stay on the
  store in release 2. They change nothing, so they cannot skip the policy.
  This PR changes section 8.1 of specification 2 to say so: the exception
  of release 1 stays for the read verbs, and each verb that adds, submits or
  withdraws an action goes through the socket.
- A wait in `submit` for the apply. `submit` prints the state of each
  action at the end of the submit.

## Design

### The socket

1. At the start, `iris serve` takes the engine lock: an exclusive lock on
   `<data>/engine.lock` (`iris_store::FileLock`). If another process holds
   it, the start fails with "another engine runs on this data folder". The
   engine holds the lock until it stops. Only the holder of the lock does
   the next steps, so two engines cannot both remove and bind the socket.
2. With the lock, `iris serve` checks `<data>/iris.sock`:
   - If the file exists, it is left from a stopped engine, because a running
     engine holds the lock. The engine removes it.
3. The engine binds the socket, then sets the mode of the file to `0600`.
   The data folder has the mode `0700` (specification 1, section 5.1), so no
   other OS user can reach the file between the bind and the mode change.
4. At a clean stop (Ctrl-C), the engine removes the file. After a crash, the
   OS releases the lock, and the next start removes the file (step 2).
5. The engine reads the user ID of the peer of each connection (`SO_PEERCRED`
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
| `drafts` | `{}` | Answer: `{"changesets": ["cs-12", ...]}`, each changeset of the author `local` whose newest revision is a draft, the newest first, at most 200. |
| `submit` | `{"changeset", "why"}` | Submits the draft revision. Answer: the changeset document after the submit. |
| `withdraw` | `{"changeset"}` | Withdraws a changeset of the same author (section 5.5). Answer: the changeset document after the withdraw. |

Answer:

```json
{"ok": true, "result": {...}}
{"ok": false, "code": 2, "error": "the reason"}
```

`code` is the exit code of section 8.4 that the command uses. The engine
gives 2 for a request that is not valid (an unknown verb, a target that is not
an ID, a changeset of another author) and 1 for an error in the store.

For `submit`, the command computes the code from the decision of each action
in the document (`decision.state` and `decision.by`), in the order of
section 8.4:

| Decision | Code |
|---|---|
| `denied` by `engine` (the submit found a target or an argument that is not valid) | 2 |
| `failed` | 1 |
| `denied` by `limit`, `rule` or `policy` | 3 |
| `proposed`, `in_review`, `approved` or `held` | 5 |
| all `applied` | 0 |

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

The submit and the decisions of the policy run in one transaction, as in
triage today: if the engine stops before the end, nothing of the submit
stays, and the draft can be submitted again.

After `submit`, the policy decides each `proposed` action (section 5.4, step
5). Today only `iris-triage` has this step, for its own author. The build
moves it to one shared function, `decide_submitted(c, changeset, revision,
author, now)`, which triage and the socket both call:

- In the mode `apply`, the active policy decides each action with the facts
  of its messages (`action_message`, also the origin messages of an item
  target), as triage does today: `approved`, `in_review` or `denied`, by a
  rule, the policy default or a hard limit.
- In the mode `observe`, a hard limit that denies (section 6.5, for example a
  verb other than `archive`, `trash` or `label` on hostile mail by an author
  other than `person`) denies the action, by `limit`. Each other action stays
  `proposed` for the person. This also applies to triage, which today keeps
  such an action `proposed` in the mode `observe`.

### The command

The verbs that add an action:

| Command | Action |
|---|---|
| `iris close <id> [--reason <text>]` | `close`, args `{"reason"}`. The default reason is "Done.". |
| `iris decline <id> [--reason <text>]` | `decline`, args `{"reason"}`. The default reason is "Declined.". |
| `iris defer <id> --date <YYYY-MM-DD>` | `defer`, args `{"date"}`. |
| `iris correct <id> --field <path> --value <json>` | `correct`, args `{"field", "value"}`. A `--value` that is not JSON is a string. |
| `iris archive`, `inbox`, `read`, `unread`, `trash` `<ids>...` | The verb, args `{}`. |
| `iris label`, `unlabel` `<ids>... --label <name>` | The verb, args `{"label"}`. |

Each one also takes `--changeset <cs-id>` and `--why <text>` (the reason of
the action). The engine checks the targets and the arguments at the submit,
as for each author.

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
| The engine stops after the transaction and before the answer, or the answer takes more than 30 s | The command exits 1: "the result is not known: run `iris changeset <cs-id>`". For an `add` with no `--changeset`, the command has no ID, so it says "run `iris changeset --drafts`", which lists the drafts of the author `local`. The draft can hold the action. A draft has no effect before `submit`, so a second draft from a retry changes nothing outside Iris; the caller withdraws it or leaves it. | A test that closes the connection before the answer. |
| The engine stops before the transaction ends | SQLite rolls back. Nothing is written. Same message as above. | Covered by the store tests of transactions. |
| Two commands add to the same draft at the same time | SQLite runs the two transactions one after the other. Each gets its own action number. | A test with two threads. |
| A changeset of another author (`agent`, `person`, `triage`) | Code 2, "only the author can do this" (`check_author`). | A test. |
| Two engines start at the same time on one data folder | One takes `engine.lock`. The other fails before it touches the socket. | A test that holds the lock and starts an engine. |
| A `local` action on hostile mail with a verb other than `archive`, `trash` or `label` | Denied by `limit` at the submit, in both modes. Exit 3. | A test in each mode. |

All other input: code 2 and no write.

## Impact

- `crates/iris/src/socket.rs` (new): the listener, the protocol and the work.
- `crates/iris-triage/src/lib.rs`: step 5 moves to the shared function. In
  the mode `observe`, a hard limit that denies now denies a triage action.
- `docs/spec/02-changeset-policy-ledger.md`: section 8.1 (the read verbs keep
  the store; the engine lock) and section 5.4 (step 5 in the mode
  `observe`).
- `crates/iris/src/lib.rs`: the new subcommands, the client, and the start of
  the listener in `serve`.
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

- With `iris serve` running, `iris close c-7 --reason "Sent it."` prints `cs-N 1`, and
  `iris changeset cs-N` shows a draft action `close` with the target `c-7`.
  Red if the command reads the store, or if no draft is made.
- `iris submit cs-N --why "Done."` prints the state of each action and exits
  5 while the action waits for the person. Red if it exits 0.
- `iris close m-1` (a message, not an item) then `iris submit` exits 2, with
  the reason of the engine. Red if it exits 3.
- `iris withdraw cs-N` makes each open action `cancelled`. Red if an applied
  action changes.
- With no engine, each of these verbs exits 4. Red if one opens the store.
- `ls -l <data>/iris.sock` shows `srw-------`. Red with another mode.
