# Spec: archive, inbox and trash for an IMAP account

- Date: 2026-11-25
- Card: none (open-issues sweep, group imap; issues #775 and #620)
- Status: Draft

## Problem

An IMAP account cannot archive, move to the inbox, trash or untrash a
message (issue #775). The IMAP provider refuses each move before any
write, because an IMAP message ID is a location ID: folder, `UIDVALIDITY`
and UID (`docs/specs/2026-11-17-provider.md`, section 4). A move gives the
message a new ID. The next sync marks the old store row gone and adds a new
row, so the ledger and the undo lose the message, and the forms of the old
row stay on a gone row.

Thus the triage proposals of an IMAP account (archive of feed, records and
hostile mail) cannot apply. Triage is the main work of Iris.

## Goal

- On an IMAP server with `MOVE` (RFC 6851) and `UIDPLUS` (RFC 4315), the
  verbs `archive`, `inbox`, `trash` and `untrash` apply.
- After a move, the store row of the message stays the same row. Its
  provider ID changes to the new location ID (the rebind). The forms, the
  ledger rows and the undo stay on that row.
- A move with an unknown result (a lost connection, a crash) ends in
  `unknown`, as a Gmail call with an unknown result does today. No move
  runs two times, and the recovery never rebinds a row.

## Not in scope

- A move into or out of a user folder. The rules of the provider spec,
  section 4, stay: a folder provider moves only between `INBOX`, the
  archive folder and the trash folder, and a move of a message in a user
  folder is a definite rejection. No verb of Iris files mail into a user
  folder: `label` on IMAP is a keyword.
- A server without `MOVE` or without `UIDPLUS`. Each move stays a definite
  rejection there (`Api` 400 `notSupported`).
- `COPY`, `STORE \Deleted` and `EXPUNGE` in place of `MOVE`. A crash
  between the steps leaves two copies, and `EXPUNGE` without `UID EXPUNGE`
  can remove other deleted mail.

## Design

### 1. The folders of a move

| Role | Folder |
|---|---|
| Inbox | `INBOX` |
| Archive | the folder with `\Archive` (RFC 6154). If no folder has it, a folder with no attribute whose last name part is a common archive name (`Archive`, `Archives`, `Archiv`, `Archivio`, `Archivo`, `Arquivo`, `Archief`, `Arkiv`), with the same rules as the other names of section 4 of the IMAP provider spec. Else none |
| Trash | the trash folder of section 4 of the IMAP provider spec |

A message of a folder with `\Archive`, or of the archive folder by name,
has the label of that folder and no `INBOX`, so the mirror shows it as
archived. Labels do not change.

| Verb | Trait call | Source | Target | No target folder |
|---|---|---|---|---|
| `archive` | `modify(remove: INBOX)` | `INBOX` | archive | `Api` 400 `noArchive`, before any write |
| `inbox` | `modify(add: INBOX)` | archive | `INBOX` | `Api` 400 `noArchive` |
| `trash` | `trash` | `INBOX` or archive | trash | `Api` 400 `noTrash` |
| `untrash` | `untrash` | trash | `INBOX` | `Api` 400 `noTrash` |

A `modify` of `INBOX` for a message that is already at the target (an
`archive` of a message in the archive folder) writes nothing for that
message, as Gmail does. A message in any other folder (a user folder,
`\Sent`, `\Drafts`, `\Junk`) is `Api` 400 `notSupported` for the whole call,
before any write, as today. A `modify` that moves and also changes flags is
`Api` 400 `badRequest`: the apply sends each verb as its own call, so no
caller does this.

### 2. The provider call

A new trait method of `iris_provider::Provider`:

```rust
/// True if a move gives a message a new ID (IMAP). Then the apply calls
/// `relocate` for each move and rebinds each store row.
fn location_ids(&self) -> bool { false }

/// Moves each message of `ids` as `modify` would with `add` and `remove`
/// (only `INBOX`), and gives the new ID of each, in the order of `ids`.
/// `None`: the message was already at the target and did not move.
fn relocate(&self, token: &str, ids: &[&str], add: &[&str], remove: &[&str])
    -> Result<Vec<Option<String>>>;
```

The default of `relocate` is `Api` 400 `notSupported`. `modify` of IMAP
keeps its refusal of `INBOX`, so a caller that does not know the new rule
cannot move mail. `trash` and `untrash` of IMAP move one message and give
a `Minimal` whose `id` is the new ID.

For each source folder, in the page order of section 2 of the IMAP provider
spec, the IMAP provider does these steps:

1. A check pass over all folders first, with no write: `SELECT` of each
   source folder, the `UIDVALIDITY` of each ID, and the target folder of
   each message (section 1). Any refusal of this pass is definite.
2. `SELECT` of the source folder, the `UIDVALIDITY` again, then
   `UID MOVE <uids> <target>`.
3. The `OK` answer must hold `[COPYUID <validity> <source uids> <target uids>]`
   (RFC 4315 section 3). Each target UID with the target `UIDVALIDITY` and
   the target folder name gives the new ID.

`UID MOVE` moves only UIDs that are in the selected source folder now. So
the source folder is the condition of the move (provider spec, section 4):
a message that another client moved after the check does not move.

Result rules:

- Each asked UID in `COPYUID`: success, with the new IDs.
- An `OK` with no `COPYUID`, or with a `COPYUID` that does not hold each
  asked UID, is `Transport` (unknown). A UID that is not there was moved or
  removed by another client, or the server is broken. The recovery decides.
- A `NO` or `BAD` to `UID MOVE` is `Transport` (unknown): RFC 6851 lets a
  server move a part of the set before an error.
- A refusal before the first `UID MOVE` was sent is definite, as for
  `modify` today. An error after it is `Transport` (unknown).

### 3. The apply: the rebind

`iris-apply` calls `relocate` in place of `modify` for each change that
`moves()` when `location_ids()` is true. On success, one store transaction
does these steps for each message of the call:

1. Sets the ledger message row `done`, as today.
2. With a new ID: sets `message.provider_id` of the row to the new ID
   (`Store::rebind`). The row ID, the thread, the raw file
   (`content_sha256`), the forms and the ledger rows stay.

Then the apply reads the labels of each new ID (`minimal`), as it does for
Outlook moves (`mirror_read`). `trash` and `untrash` rebind to the `id` of
the `Minimal` that they give, in the same transaction as `done`.

`Store::rebind` refuses a new ID that another row of the account already
has. That can happen only if a sync stored the new location first, and the
apply holds the sync lock of the account (`sync_lock`), so it is an error
of the store (`Error::Store`), and the apply stops.

The next sync then sees:

- `Deleted` of the old ID: no row has it, so nothing changes (iris-sync
  `mark_gone` of an unknown ID).
- `Added` of the new ID: the row exists and was fetched, so the sync sets
  its labels and fetches nothing.
- With `CONDSTORE` only, the removal from the source folder gives
  `HistoryExpired` (IMAP provider spec, section 5), and the full resync
  matches the new ID to the same row.

### 4. The recovery

A move with an unknown result, or a crash after `UID MOVE` and before the
transaction of section 3, leaves the ledger message rows not `done`. The
recovery (spec 2 section 7.3) handles them with the rules of today for a
call that started:

- The recovery never calls `UID MOVE` again, and never rebinds a row. Only
  the `COPYUID` of the same command names the moved message with
  certainty. A `Message-ID` match does not: the target folder can hold
  another copy with the same `Message-ID`, so a rebind by `Message-ID`
  could put the forms and the undo on the wrong message.
- A started move whose old ID is still there with the labels of before
  did not move, but the recovery cannot tell it from a move that another
  client undid. The row gets `listed = 'unknown'`, as today for a started
  call with the labels of before. The ledger entry ends `failed`, and the
  action shows the message as not confirmed. Iris does not call again.
- A started move whose old ID is gone gets `listed = 'unknown'` too.

After a crash, the sync lock ends with the process, and the engine can
sync before the recovery. The sync then marks the old row gone and adds a
new row for the new ID. The recovery finds the old row gone (`listed =
'gone'`, as today). So after a crash or an unknown result, the forms of
the message stay on the gone row, the new row is understood again, and
the undo of that entry cannot move the message back. This is the cost of
an unknown result: limit issue #984. The success path
of section 3 has no such cost, because the rebind and `done` are one
transaction under the sync lock.

### 5. The undo

The undo of each move is a move back (provider spec, section 4). It finds
the message by its store row, and the row has the current provider ID after
the rebind. `undo` of `archive` is `inbox`, and the undo of `trash` is
`untrash` and then `archive` of each message that had no `INBOX` before, as
today (spec 2 section 7.4).

## Options not chosen

- Match the new row to the old row by `Message-ID`, at the next sync or
  in the recovery: the target folder can hold another copy with the same
  `Message-ID`, so the match can put the forms and the undo on the wrong
  message.
- `OBJECTID` (RFC 8474) `EMAILID` as the message ID: an `EMAILID` alone
  cannot fetch a message, and few servers have it.
- `COPY` then `UID STORE +FLAGS \Deleted` then `UID EXPUNGE`: three writes,
  each with an unknown result, and a crash between them leaves two copies.
- A library: the IMAP client (`crates/iris-imap`) keeps its own command
  framing and uses `imap-proto` 0.17 for the answers (IMAP client spec,
  "Options not chosen"). `imap-proto` parses `COPYUID` as a response code
  (`ResponseCode::CopyUid`), so the design writes no new parser.

## Failure list

Supported inputs: an IMAP server with the capabilities `MOVE` and
`UIDPLUS`, and IDs that `minimal`, `messages` and `changes` gave.
The safe result for all other input: a definite rejection before any
write, or `Transport` (unknown) after the first write.

| Input or event | Result | Test |
|---|---|---|
| No `MOVE` or no `UIDPLUS` | `Api` 400 `notSupported`, nothing sent | `move_needs_move_and_uidplus` |
| No archive folder, for `archive` or `inbox` | `Api` 400 `noArchive`, nothing sent | `move_without_archive_folder` |
| No trash folder | `Api` 400 `noTrash`, nothing sent | `move_without_archive_folder` |
| A message in a user folder, `\Sent`, `\Drafts` or `\Junk` | `Api` 400 `notSupported`, nothing sent | `move_from_other_folder_refuses` |
| A new `UIDVALIDITY` before the first `UID MOVE` | `NotFound`, nothing sent | `move_uidvalidity` |
| A new `UIDVALIDITY` after a `UID MOVE` of another folder | `Transport`, no more writes | `move_uidvalidity` |
| `OK` with no `COPYUID`, or without each asked UID | `Transport` | `move_partial_copyuid` |
| `NO` or `BAD` to `UID MOVE` | `Transport` | `move_refused_is_unknown` |
| A lost connection after `UID MOVE` | `Transport`; the recovery sets `listed = 'unknown'`, the entry ends `failed`, no second `UID MOVE`, no rebind | `move_unknown_is_not_retried` |
| A started move whose old ID still has the labels of before | `listed = 'unknown'`, entry `failed`, no call | `move_unknown_is_not_retried` |
| A crash after `UID MOVE`, then a sync before the recovery | the sync marks the old row gone and adds a new row; the recovery sets `listed = 'gone'`; zero differences | `move_crash_then_sync` |
| A message that is already at the target | `None`, no write for it | `move_already_there` |
| `archive`, sync, `inbox`, sync, then undo of both | the same store row, zero differences | `move_and_undo_keep_the_row` |
| A sync after the move (QRESYNC, and CONDSTORE only) | no new row, no gone row, zero differences | `move_then_sync_keeps_the_row` |
| `Store::rebind` to an ID that another row has | `Error::Store`, the apply stops | `rebind_refuses_a_taken_id` |

## Impact

- `crates/iris-provider`: `location_ids` and `relocate`, with defaults.
- `crates/iris-imap`: `Session::move_uids` (`UID MOVE`, `COPYUID`), the
  archive folder, `relocate`, `trash`, `untrash`. The Gmail refusal comes
  before each other check of `profile` (issue #981).
- `crates/iris-fakeimap`: the capabilities `MOVE` and `UIDPLUS`, `UID MOVE`
  with `COPYUID`, and a switch to drop the answer after
  a `UID MOVE` (the unknown result).
- `crates/iris-store`: `Store::rebind`. No migration.
- `crates/iris-apply`: `relocate` in place of `modify` for moves, and the
  rebind in the `done` transaction. The recovery does not change.
- Docs: the provider spec section 4, the IMAP provider spec section 7, and
  the README IMAP line.

## Assumptions

| Assumption | Evidence |
|---|---|
| The common IMAP servers with `MOVE` also have `UIDPLUS` (Dovecot, Fastmail, Cyrus, Gmail) | pending. A server without one of them keeps the refusal of today, so a wrong assumption only keeps #775 open for that server |
| `UID MOVE` moves only UIDs that are in the selected folder | RFC 6851 section 3.3, and RFC 3501 section 6.4.8 for `UID` commands |
| The `OK` of `UID MOVE` holds `COPYUID` on a `UIDPLUS` server | RFC 6851 section 4.3. Else the result is unknown (section 2), so a wrong assumption gives `unknown`, never a wrong rebind |
| `imap-proto` 0.17 parses `COPYUID` | `ResponseCode::CopyUid(u32, Vec<UidSetMember>, Vec<UidSetMember>)` in `imap-proto-0.17.0/src/rfc3501/mod.rs`, with a test in its `tests.rs` |
| `mark_gone` of an ID with no row changes nothing | `crates/iris-sync/src/lib.rs`, `apply`, `Change::Deleted` |
| An `Added` of an ID that a fetched row has sets only its labels | `crates/iris-sync/src/lib.rs`, `apply`, the `row` lookup |
| The apply holds the sync lock of the account | `crates/iris-apply/src/lib.rs`, `sync_lock` in `apply` and `recover` |

## External writes

`UID MOVE` (section 2), one for each source folder of a call.

- Success: the new IDs, the rebind and `done` in one transaction.
- Definite rejection (any refusal before the first `UID MOVE`): `failed`,
  nothing written, as today.
- Unknown result (a lost answer, `NO`, `BAD`, a partial `COPYUID`, a crash):
  the recovery of section 4.
- Retry: none. The apply never sends `UID MOVE` again for a ledger row that
  started, and the recovery only reads.

## Open questions

None.

## Acceptance criteria

- On the fake IMAP server with `MOVE` and `UIDPLUS`, triage `archive`,
  `inbox`, `trash` and `untrash` apply, and the store row of each message
  stays the same row. Red if the rebind is missing (the sync makes a new
  row).
- The undo of each move brings the message back to its folder with zero
  differences. Red if the undo uses the old ID.
- A lost answer after `UID MOVE` ends `failed` with the message
  `unknown`, and no second `UID MOVE`. Red if the recovery calls
  `UID MOVE` again or rebinds a row.
- A server without `MOVE` or `UIDPLUS` refuses each move before any write.

## Verify

```bash
cargo test -p iris-imap -p iris-apply -p iris-store -p iris-provider
scripts/local-ci/run.sh
```

## Rollout

None. The change needs no migration and no setting. A server without
`MOVE` and `UIDPLUS` behaves as today.

## Risks

- A wrong rebind would point a row at another message. The rebind uses
  only the `COPYUID` of the same command.
- Rollback: a revert. A row that was rebound keeps its new ID, which is the
  real location of the message, so the old code syncs it as before.

## Plan

1. Spec (this PR).
2. `iris-provider`, `iris-imap`, `iris-fakeimap`: `relocate`, `trash`,
   `untrash` and the fake server, and the fix of #981.
3. `iris-store`, `iris-apply`: the rebind and the recovery, with the
   end-to-end tests of the failure list. Close #775.

## Test plan

The tests of the failure list. The end-to-end tests run iris-apply on the
fake IMAP server and check zero differences with `iris_sync::verify`.

## Spec checks

- [x] Each assumption holds with a second caller: the sync lock stops a
  sync between the move and the rebind; another IMAP client can move the
  message, and then `COPYUID` does not hold it (unknown).
- [x] Each assumption holds when an outcome is unknown: section 4, with
  the crash and the sync before the recovery.
- [x] Each dropped mechanism was dropped for a reason: `COPY` and
  `EXPUNGE` (two copies after a crash).
- [x] Each "idempotent" step is not check-then-act: `UID MOVE` is never
  sent two times; the recovery only reads and never rebinds.
- [x] Each property has a test: the failure list.
- [x] The spec addresses the hardest problem: the new ID after a move, and
  the unknown result.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result.
