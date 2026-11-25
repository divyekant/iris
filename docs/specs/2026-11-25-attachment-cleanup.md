# Spec: cleanup of the attachment folder

- Date: 2026-11-25
- Card: none (limit issue #853)
- Status: Draft

## Problem

`POST /api/attachments` writes each uploaded file to the attachment folder
of the store, `attachments/<aa>/<sha256>` (send content spec
`docs/specs/2026-11-24-send-content.md`, section 2). Nothing deletes a file
of this folder. The folder grows with each upload, also for a draft that
the person did not send, and for a draft that the person removed.

A file that an open draft action names must stay. If it is missing, the
apply denies the action ("an attachment file is missing or changed"). So a
wrong delete does not send a wrong mail, but the person loses the file of
an open draft, and must attach it again.

## Goal

1. The engine deletes each attachment file that no live reference names
   and that is older than a keep period.
2. A file that a live draft or a live action names is never deleted.
3. A file that the person uploaded and did not submit yet stays for the
   keep period.

## Not in scope

- A cleanup on the removal of an account (D34). The rule of this spec
  applies there too, because a removed account has no open actions.
- A limit on the size of the folder.
- A setting for the keep period.

## Design

### 1. Live references

A file is live when its SHA-256 is in one of these places:

| Place | Why the file is needed |
|---|---|
| `changeset_action.args`, `$.attachments[*].sha256`, of a `draft` action whose state is open: `NULL` (a draft revision), `proposed`, `in_review`, `approved` or `held` | The apply reads each file. |
| `ledger.post`, `$.content.attachments[*].sha256`, of a ledger row in the state `writing` | Recovery makes the same bytes again (send spec, section 4, step 2). |
| `draft.content`, `$.content.attachments[*].sha256`, of a draft row with `sent_at IS NULL` and `foreign_at IS NULL` | The client can edit the draft. The edit submits a new `draft` action with the same entries, and its apply reads the files. |
| `ledger.pre`, `$.draft.content.attachments[*].sha256`, of a draft change entry whose undo is available (view `ledger_can_undo`) | The undo of the change makes the old content again, and its apply reads the files (draft change spec). |

The JSON of these places is input that Iris does not control: `iris
draft` saves a draft revision before submit checks its content. The
cleanup reads a place only when it is valid JSON with an array at the
path, and an entry only when it is an object with a text `sha256`. Any
other value names no file and does not stop the cleanup.

A decided action (`applied`, `failed`, `denied`, `expired`, `cancelled`,
`replaced`, `undone`) never applies again, so it does not keep a file. A
sent draft and a foreign draft (send spec, section 4.1) never change
again.

### 2. The keep period

A file is old when its modification time is 30 days (`GONE_KEEP_MS`) or
more before the time of the cleanup. The writer of the folder
(`ContentWriter`) renames a new temporary file over the file at each
upload, so an upload of the same bytes makes the file new again. The keep
period covers a file that the client holds in its form and did not
submit yet. The action expiry is 14 days, so a submitted action that
names a file decides before the file is old.

### 3. The cleanup

`Store::clean_attachments(now)`:

1. Take the backup lock (as `clean_gone`). If a backup runs, do nothing
   in this tick.
2. List the files of the attachment folder. Skip each name that is not
   `<aa>/<64 hex>` and each temporary file.
3. Start one transaction with the write lock of the store
   (`Immediate`), and read the live references (section 1).
4. In the same transaction, for each file:
   1. If a live reference names its SHA-256, keep the file.
   2. If its modification time is not old, keep the file.
   3. Delete the file.
5. Give the number of deleted files.

The engine runs the cleanup each `CLEAN_EVERY` (one hour), in the tick,
after `clean_gone`. An error is logged, and the next run tries again.

### 4. Concurrency

`put_attachment` renames its temporary file into the folder while it
holds the write lock of the store. So an upload and step 3 of the
cleanup do not overlap:

- If the upload renames first, the file is new, and step 4.2 keeps it.
- If the cleanup deletes first, the rename then makes a new file.

A submit writes the action in a write transaction. So step 3 sees
each committed action. If the cleanup deletes a file before the submit
of an action that names it, the apply denies the action, and the person
attaches the file again. This happens only for a file that the client
held for 30 days or more.

## Options not chosen

- A reference count column for each file: each writer of `args`,
  `post` and `draft.content` must keep it current. A missed writer
  deletes a live file. The scan of section 1 reads the source rows.
- Delete at once when an action decides: a draft row or another action
  can name the same file, and the client can hold the file in its form.
- No keep period: a file that the person uploaded seconds ago is not yet
  in an action.

No library applies: the change is a store query and a file delete.

## Failure list

| Input or event | Result | Test |
|---|---|---|
| A file that an open `draft` action names (a revision, `approved`) | Kept, at any age | `only_old_dead_files_go` |
| A file that a ledger row in `writing` names | Kept | `only_old_dead_files_go` |
| A file that an unsent draft row names | Kept | `only_old_dead_files_go` |
| A file that only a decided action, a sent draft or a foreign draft names | Deleted when old | `only_old_dead_files_go` |
| A new file that nothing names | Kept until old | `only_old_dead_files_go` |
| An upload of the same bytes while the cleanup runs | The file stays | `an_upload_makes_a_file_new` |
| A name that is not `<aa>/<64 hex>`, a file in the wrong folder, a temporary file | Not deleted | `only_old_dead_files_go` |
| No attachment folder | Nothing to do | `no_folder_is_no_work` |
| A backup that runs | No delete in this run | `a_backup_stops_the_cleanup` |
| A crash during the cleanup | The cleanup changes no rows. A file is deleted or not. The next run continues. | none |
| An error to read the folder or to delete a file | The error is logged. No row changes. | none |

## Impact

- `crates/iris-store/src/backup.rs`: `clean_attachments`.
- `crates/iris-store/src/lib.rs`: `put_attachment` renames under the
  write lock. The comment "Nothing deletes a file" changes.
- `crates/iris/src/engine.rs`: the call each `CLEAN_EVERY`.
- `docs/specs/2026-11-24-send-content.md`, section 2: the cleanup line.
- No schema change. No API change. No screen change.

## Assumptions

1. `changeset_action.args` of a `draft` action is the content JSON with
   `attachments`. Evidence: `crates/iris-apply/src/send.rs`, the parse of
   `args` as `Content`.
2. The ledger `post` of a draft holds the content. Evidence: `post.content`
   in `crates/iris-apply/src/send.rs`.
3. A decided action does not apply again. Evidence: the trigger
   `action_state` of `crates/iris-store/src/schema_11.sql`.
4. `ContentWriter::finish` renames a new temporary file over a file that
   exists, so the file gets a new modification time. Evidence:
   `crates/iris-store/src/content.rs`, and the test
   `an_upload_makes_a_file_new`.

## External writes

None. The cleanup deletes local files only.

## Open questions

None.

## Acceptance criteria

1. Each test of the failure list passes. A cleanup that ignores one of
   the three places of section 1 makes its test red.
2. The send tests and the backup tests pass with no change.

## Verify

```bash
cargo test -p iris-store --test attachments
scripts/local-ci/run.sh
```

## Rollout

None. The first run deletes the old files that nothing names.

## Risks

A missed reference deletes the file of an open draft. The apply then
denies the action, and the person attaches the file again. No wrong mail
goes out. Rollback: revert the engine call.

## Plan

1. #853: the cleanup, its tests and the spec change, in one PR.
