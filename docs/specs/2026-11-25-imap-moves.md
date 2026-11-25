# Spec: archive, inbox and trash for an IMAP account

- Date: 2026-11-25
- Card: none (open-issues sweep, group imap; issues #775 and #620)
- Status: Built

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
| Archive | the folder with `\Archive` (RFC 6154). If no folder has it, a folder with no attribute whose last name part is a common archive name (`Archive`, `Archives`, `Archiv`, `Archivio`, `Archivo`, `Arquivo`, `Archief`, `Arkiv`), with the same rules as the other names of section 4 of the IMAP provider spec |
| Trash | the folder with `\Trash`, else a folder by a trash name (section 4 of the IMAP provider spec) |

The plan first sorts each message by the role of its folder (the table
below): moves, skip, or refused. The role of a folder is the role that the
reads already give it (`Folder::special` after `by_name`, IMAP provider
spec section 4): a folder with the attribute of the role, and a folder by
name only if no folder has that attribute. So the plan and the labels
agree: a message counts as in the trash only if it has the label `TRASH`.
A message in either of two trash folders counts as in the trash. Only if at least one message moves does the plan need the target
folder: exactly one folder of the target role. Two such folders give
`Api` 400 `ambiguousArchive` or `ambiguousTrash`, and none gives `noArchive`
or `noTrash`, before any write. An action whose messages all skip needs no
target folder and writes nothing. The IMAP provider spec lets two trash
folders both give `TRASH`; that rule stays for reads.

A message of the archive folder has the label of that folder and no
`INBOX`, so the mirror shows it as archived. Labels do not change.

The source of each message comes from its ID: an IMAP ID holds its folder
(IMAP provider spec, section 3). So the plan of a move needs no read of a
message.

| Verb | Moves from | Already done (skip) | Target |
|---|---|---|---|
| `archive` | `INBOX` | an archive folder, a trash folder | archive |
| `inbox` | an archive folder | `INBOX` | `INBOX` |
| `trash` | `INBOX`, an archive folder | a trash folder | trash |
| `untrash` | a trash folder | `INBOX`, an archive folder | `INBOX` |

A message in any other folder (a user folder, `\Sent`, `\Drafts`,
`\Junk`) refuses the whole action with `Api` 400
`notSupported`, before any write and before the claim of the ledger
(section 3).

### 2. The provider calls

New items of `iris_provider::Provider`, each with a default:

```rust
/// The four moves of a folder provider with location IDs.
pub enum Move { Archive, Inbox, Trash, Untrash }

/// True if a move gives a message a new ID (IMAP). Default: false.
fn location_ids(&self) -> bool { false }

/// Reads only. For each message of `ids`, in order: true if `mv` moves it,
/// false if it is already done (the skip column of section 1). Any message
/// that `mv` cannot move or skip, and a missing or ambiguous folder, is a
/// definite refusal. Default: `Api` 400 `notSupported`.
fn move_plan(&self, token: &str, ids: &[&str], mv: Move) -> Result<Vec<bool>>;

/// Moves each message of `ids` and gives its new ID, in the order of
/// `ids`. Each ID must be a "moves from" ID of `mv`, else a definite
/// refusal before any write. Default: `Api` 400 `notSupported`.
fn relocate(&self, token: &str, ids: &[&str], mv: Move) -> Result<Vec<String>>;
```

`modify`, `trash` and `untrash` of IMAP keep their refusal of each move, so
a caller that does not know the new rule cannot move mail.

`relocate` of the IMAP provider does these steps:

1. `LIST`, the folders of section 1, and a check of each ID (its folder is
   a "moves from" folder of `mv`). Any refusal here is definite.
2. For each source folder, in the page order of section 2 of the IMAP
   provider spec: `SELECT`, a compare of its `UIDVALIDITY` with each ID of
   that folder, then `UID MOVE <uids> <target>`. A new `UIDVALIDITY` before
   the first `UID MOVE` is `NotFound`, nothing written. After a `UID MOVE`
   of an earlier folder it is `Transport` (unknown), and nothing more is
   written.
3. The `OK` answer must hold `[COPYUID <validity> <source uids> <target uids>]`
   (RFC 4315 section 3). Each target UID with the target `UIDVALIDITY` and
   the target folder name gives the new ID.
   Each `COPYUID` of one call must give the same target `UIDVALIDITY`. A
   change makes the new IDs of the earlier folders stale, so the result is
   `Transport` (unknown).

`UID MOVE` moves only UIDs that are in the selected source folder now. So
the source folder is the condition of the move (provider spec, section 4):
a message that another client moved after the plan does not move.

Result rules:

- Each asked UID in `COPYUID`: success, with the new IDs.
- An `OK` with no `COPYUID`, or with a `COPYUID` that does not hold each
  asked UID, is `Transport` (unknown). A UID that is not there was moved or
  removed by another client, or the server is broken.
- A `NO` or `BAD` to `UID MOVE` is `Transport` (unknown): RFC 6851 lets a
  server move a part of the set before an error.
- A refusal before the first `UID MOVE` was sent is definite, as for
  `modify` today. An error after it is `Transport` (unknown).

### 3. The apply: the plan and the rebind

When `location_ids()` is true, `iris-apply` uses the two calls for the
verbs `archive`, `inbox`, `trash` and `untrash`:

- Step 2 (the read before the claim) calls `move_plan` for the messages
  that are not gone. It skips each message with `false`, as it skips a
  message that already has the target state today. A definite refusal
  ends the action with `Run::fail_unclaimed`, as a refused label create
  does today: one transaction, with the checks of the claim (the mode and
  the expiry), makes a ledger entry with no message in `failed`, so the
  `action_state` trigger moves the action to `failed`. No message moves,
  and no ledger message row says that one did. An unknown error of the
  plan read (`Transport`) stops the apply before the claim, with no ledger
  entry; the action stays approved, and the next apply reads again,
  because the plan writes nothing. The label read of step 2 stays for the
  `pre` labels of the ledger.
- Step 4 calls `relocate` in place of `modify`, `trash` and `untrash`: one
  call for each batch of the moved messages.

On success, one store transaction does these steps for each message of the
call:

1. Sets the ledger message row `done`, as today.
2. Sets `message.provider_id` of the row to the new ID (`Store::rebind`).
   The row ID, the raw file (`content_sha256`), the forms and the ledger
   rows stay.
3. If the provider ID of the thread of the row is the old message ID (the
   thread of a message with no `Message-ID`, `References` or
   `In-Reply-To`: IMAP provider spec, section 3), it sets the provider ID
   of that thread to the new ID too. Such a thread has only this message,
   because no other message can refer to a message with no `Message-ID`.
   So the thread that `iris_sync::verify` reads from the provider and the
   thread in the store stay equal.

Then the apply reads the labels of each new ID (`minimal`), as it does for
Outlook moves (`mirror_read`).

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
recovery (spec 2 section 7.3) gets one new rule for these rows:

- On a provider with `location_ids()`, each ledger message row of a move
  whose call started (its call number is saved) and that is not `done`
  gets `listed = 'unknown'`, whatever the read of its old ID gives: the
  labels of before, `NotFound`, or a store row that a sync marked gone.
  The ledger entry ends `failed`, and the action shows the messages as not
  confirmed.
- Today a row whose message is `NotFound` or gone gets `listed = 'gone'`,
  and an entry with only such rows ends `applied`. For a move with location
  IDs, `NotFound` is what a successful move gives, so `gone` would report
  an unconfirmed move as applied.
- The recovery never calls `UID MOVE` again, and never rebinds a row. Only
  the `COPYUID` of the same command names the moved message with
  certainty. A `Message-ID` match does not: the target folder can hold
  another copy with the same `Message-ID`, so a rebind by `Message-ID`
  could put the forms and the undo on the wrong message.
- A row of a call that never started keeps the rule of today.

After a crash, the sync lock ends with the process, and the engine can
sync before the recovery. The sync then marks the old row gone and adds a
new row for the new ID. The recovery gives that row `unknown` by the rule
above. So after a crash or an unknown result, the forms of the message
stay on the gone row, the new row is understood again, and the undo of
that entry cannot move the message back: limit issue #984. The success
path of section 3 has no such cost, because the rebind and `done` are one
transaction under the sync lock.

### 5. The undo

The undo of each move is a move back (provider spec, section 4). It finds
the message by its store row, and the row has the current provider ID after
the rebind. `undo` of `archive` is `inbox`, and the undo of `trash` is
`untrash` and then `archive` of each message that had no `INBOX` before, as
today (spec 2 section 7.4).

A message that another client moved after the entry is not refused by the
plan (issue #990). Its row still has the ID of the folder that Iris moved it
to, and that ID is not found now. So step 2 counts the message gone before
`move_plan`, or, after a sync marked the row gone, the undo leaves it out.
The plan only sees IDs of folders that Iris saw, so an undo never names a
folder that its move cannot leave.

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
| No archive folder, and `archive` of a message in `INBOX` (a message that must move) | `Api` 400 `noArchive`, nothing sent | `move_without_folder` |
| No trash folder, and `trash` of a message in `INBOX` or an archive folder (a message that must move) | `Api` 400 `noTrash`, nothing sent | `move_without_folder` |
| No archive folder, and `inbox` of a message in `INBOX`; or no trash folder, and `untrash` of a message in `INBOX` | the plan gives false (skip): `inbox` and `untrash` move to `INBOX`, which each server has; nothing sent | `move_skip_needs_no_target` |
| Two archive folders or two trash folders as the target of a message that moves | `Api` 400 `ambiguousArchive` or `ambiguousTrash`, nothing sent | `move_ambiguous_folder` |
| A server with a `\Trash` folder and also an ordinary folder named `Trash`: `untrash` of a message in the ordinary folder | the ordinary folder has no role; the plan refuses with `notSupported`, nothing sent | `move_role_follows_the_attribute` |
| `archive` of a message in the trash, or `inbox` of a message in `INBOX`, on a server with no archive folder | the plan gives false (skip); no refusal, nothing sent | `move_skip_needs_no_target` |
| An action with one message in `INBOX` and one in a user folder, `\Sent`, `\Drafts` or `\Junk` | the plan refuses; `fail_unclaimed` gives a `failed` ledger entry with no message and a `failed` action; nothing moves | `move_from_other_folder_refuses` |
| A lost connection during the plan read | no ledger entry, the action stays approved, nothing moves | `move_plan_transport_error` |
| A new `UIDVALIDITY` before the first `UID MOVE` | `NotFound`, nothing sent | `move_uidvalidity` |
| A new `UIDVALIDITY` after a `UID MOVE` of another folder | `Transport`, no more writes | `move_uidvalidity` |
| A new target `UIDVALIDITY` between two `UID MOVE` commands | `Transport` (unknown) | `move_target_uidvalidity` |
| `OK` with no `COPYUID`, or without each asked UID | `Transport` | `move_partial_copyuid` |
| `NO` or `BAD` to `UID MOVE` | `Transport` | `move_partial_copyuid`, `move_unknown_is_not_retried` |
| A lost answer after a `UID MOVE` that moved the message (old ID `NotFound`) | the recovery sets `listed = 'unknown'`, the entry ends `failed`, no second `UID MOVE`, no rebind | `move_unknown_is_not_retried` |
| A started move whose old ID still has the labels of before | `listed = 'unknown'`, entry `failed`, no call | `move_unknown_is_not_retried` |
| A crash after `UID MOVE`, then a sync before the recovery | the sync marks the old row gone and adds a new row; the recovery sets `listed = 'unknown'`, entry `failed`; zero differences | `move_crash_then_sync` |
| A move and a sync of a message with no `Message-ID`, `References` or `In-Reply-To` | the thread follows the new ID; zero differences | `move_of_a_message_with_no_thread_headers` |
| A message that is already at the target | the plan gives false, the apply skips it, no ledger row for it | `move_already_there` |
| `archive`, sync, `inbox`, sync, then undo of both | the same store row, zero differences | `move_and_undo_keep_the_row` |
| A sync after the move (QRESYNC, and CONDSTORE only) | no new row, no gone row, zero differences | `move_and_undo_keep_the_row` |
| Another client moves an archived message to the trash, then the undo of the archive | the undo counts that message gone or leaves it out (its archive ID is not found, or a sync marked the row gone); the other messages move back; zero differences | `undo_after_an_external_move` |
| `Store::rebind` to an ID that another row has | `Error::Store`, the apply stops | `rebind_refuses_a_taken_id` |

## Impact

- `crates/iris-provider`: `Move`, `location_ids`, `move_plan` and `relocate`, with defaults.
- `crates/iris-imap`: `Session::move_uids` (`UID MOVE`, `COPYUID`), the
  archive folder, `move_plan`, `relocate`. The Gmail refusal comes
  before each other check of `profile` (issue #981).
- `crates/iris-fakeimap`: the capabilities `MOVE` and `UIDPLUS`, `UID MOVE`
  with `COPYUID`, and a switch to drop the answer after
  a `UID MOVE` (the unknown result).
- `crates/iris-store`: `Store::rebind` (the message, and a thread with the fallback ID). No migration.
- `crates/iris-apply`: `move_plan` in step 2, `relocate` in step 4, the
  rebind in the `done` transaction, and the recovery rule of section 4.
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
2. `iris-provider`, `iris-imap`, `iris-fakeimap`: `Move`, `move_plan`,
   `relocate` and the fake server, and the fix of #981.
3. `iris-store`, `iris-apply`: the plan, the rebind and the recovery, with the
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
