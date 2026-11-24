# Spec: remove an account

- Date: 2026-11-24
- Card: P11-a (decision D34, issue #626)
- Status: Draft

## Problem

A person who stops using a mailbox cannot remove it from Iris. Its mail
stays in the brief, in search and on disk. The sync stops only when the
provider refuses the sign-in. Decision D34 (owner, 2026-11-24): a command and
a button remove the account and its mail from Iris, not from the provider.
This must exist before Iris tells people to connect more than one account.

The removal deletes personal data. So it needs a clear list of what goes,
what stays, and what happens after a crash during the delete.

## Goal

1. `iris account remove <address>` and a button on the Accounts page remove
   one account.
2. After the removal, Iris holds no mail of the account: no message, header,
   clean text, search text, raw message file, thread, label, item or job
   workspace file. One exception: the notes of a job (`notes.md`) are part
   of the record (see "What stays"), and a workspace gets them back.
3. Iris writes nothing to the provider. The mailbox at the provider does not
   change.
4. A crash at any point leaves a state that the engine completes, with no
   mail of the account in the brief.
5. The person can add the same address again later. It then gets a new
   first sync.

## Not in scope

Each item below has a `limit` issue (see the PR).

- A delete at the provider. Iris never deletes a mailbox.
- Backups. A backup copy that the person made (`iris backup`) and the
  migration copies `iris.db.v<N>.<time>-<hex>.bak` keep the mail of the
  account. The command and the button say so. (Limit issue to file with the
  code PR.)
- The record of actions (see "What stays"). It can hold text that the
  person or an agent wrote, and the titles of decided matters. The notes
  of a job are part of the record: an agent can quote mail in them, and
  the workspace of the job gets them back at each build. The command and
  the button say so.
- An action in review (`in_review`) of the account. Only the person can
  decide it (the trigger `action_state`). It stays in review until the
  person decides it or it expires after 14 days. An approval applies
  nothing: the tombstone never gets a sign-in again (see "Add the address
  again"), and the messages and items of its targets are gone.
- The recovery of an apply step whose provider result is unknown at the
  removal. The ledger entry stays `writing`.

## Design

### Words

- **Removal**: the step that the person starts. It is fast and atomic.
- **Delete**: the work after the removal that deletes the rows and files of
  the account. It runs in batches and can take minutes for a large mailbox.

### States of an account

Two new values of `account.state`:

| State | Meaning | Shown |
|---|---|---|
| `removing` | The removal is done. The delete did not end. | The Accounts page shows the address with "Iris deletes the mail of this account." The brief shows nothing of it. |
| `removed` | The delete ended. The row is a tombstone. | Nowhere. |

The account row stays as a tombstone: `id`, `provider` and `address`. It
holds no sign-in and no sync state. Reasons:

- `ledger.account_id` and `draft.account_id` refer to it, and the ledger
  never deletes an entry.
- `account.id` has no `AUTOINCREMENT`. A deleted row lets SQLite give its ID
  to a new account, and the old ledger entries would then name the wrong
  mailbox.
- The tombstone never gets a sign-in again. So no action of the account,
  also one that stays in review, can ever apply.

### The removal (one write transaction)

1. Refuse if the account does not exist, or its state is `removing` or
   `removed`.
2. Refuse if a session of a job is live whose scope has mail of the
   account. A live session is a session whose processes did not stop yet
   (`job_session.reaped_at IS NULL`), as in the session spec: `ended_at`
   ends only the grant, and the process of an ended session can still
   write to its workspace until the reap check passes. The agent has a copy
   of the mail in its workspace. The person must stop the job and wait for
   its end. This is the one meaning of "live session" in this spec.
3. Set `state = 'removing'`, and set each other column of the row to NULL,
   other than `id`, `provider` and `address`: the token, the IMAP server,
   port and user (`imap_host`, `imap_port`, `imap_user`), the sync state
   (`history_id`, `mirror_ready_at`, `sync_kind`, `sync_start`,
   `sync_step`) and each column that a later migration adds. The removal
   reads the column list from the schema (`pragma_table_info`), so a new
   column cannot stay by mistake. With no token, `sync_accounts` drops the
   account, so no new sync or apply starts.
4. Deny each open action (`proposed`, `approved`, `held`) that
   targets a message, an item or a draft of the account:
   `state = 'denied'`, `decided_by = 'engine'`, reason "the account was
   removed". An action in review keeps its state: only the person can
   decide it (trigger `action_state`, see "Not in scope"). An action with a
   ledger entry in state `writing` keeps its
   state, as `iris_changeset::expire` does.
5. Write one row in the new table `removed_workspace(account_id, job_id)`
   for each job whose scope has mail of the account: a scope thread of the
   account, a scope item of the account, or a listed message
   (`job_scope_message`) of the account. A scope row whose thread, item or
   listed message is gone (an item that changed, a message that
   `clean_gone` deleted) counts too, for each removal: the store keeps no
   account for the gone row, and the workspace can still hold its mail.
   After step 6 and the delete, no
   other row links the job to the account. So the delete and each build of
   the workspace read this table, also after a crash.
6. Delete the items of the account (step "items" below), so the brief is
   empty of the account at the commit. The removal transaction runs with
   `PRAGMA secure_delete = ON`, as each batch of the delete, so no free
   page keeps the text of a deleted item (issue #882).

Migration 47 adds triggers that refuse an insert into `message`, `thread`,
`label`, `item` and `body` for an account in state `removing` or `removed`.
So no writer can add mail or a derived row after the removal:

- A sync that started before the removal and holds a token in memory fails
  at its next write.
- An understanding run that read a message before the removal fails at its
  write of an item or a clean text. Its transaction rolls back. The batch
  query of understanding also leaves out the messages of such an account,
  so the next run does not try them again.

### The delete

`Store::delete_removed` runs the delete of each account in state
`removing`. It holds the sync lock of the account (`iris_sync::lock`), so no
sync or apply of the account runs at the same time, in this process or in
another. It holds the backup lock, as `clean_gone` does, so a backup does not
copy a half state. The engine tick takes the backup lock, then the sync
lock of each account, and skips a held lock: the next tick tries again.
The waiting call of the command and the API takes the sync lock of its own
account first, then the backup lock. So a wait for one account never holds
the backup lock, and never holds up the delete of another account. No
holder of the backup lock waits for a sync lock, so this order cannot
deadlock.

It runs with `PRAGMA secure_delete = ON`, so SQLite writes zeros over the
deleted content. Steps, each in batches of 500 rows, one write transaction
for each batch:

0. **Workspaces**: an agent can copy granted mail into each file of its
   workspace: a draft, a scratch file, a file with no known name. Iris
   cannot tell which file holds a copy. So the rule is one rule for the
   whole folder: a build of the workspace of a job with a row in
   `removed_workspace` deletes each entry of the folder other than
   `notes.md`, and builds the rest again. The notes are the record, so the
   delete never touches `notes.md`, and it does not depend on the result
   of the save. Order, under the build lock of the workspace:
   1. Save `notes.md` to the store (`save_notes`), as each build does. A
      save can fail (a file larger than 64 KiB, a link, a folder). Then
      the store keeps its last copy, and the file stays as it is.
   2. Delete the entries that a stopped build can leave beside the
      folder: the stage `.<name>.build` and the part file `.<name>.notes`
      (`iris_workspace::clear_beside`). They can hold a copy of the mail.
   3. Delete each entry of the folder other than `notes.md`, then the row
      of the job in `removed_workspace`.
   4. Build as usual. The grant leaves out each message of an account in
      state `removing` or `removed` (a condition in
      `iris_workspace::grant_in`). If `notes.md` is missing, it comes back
      from the store.

   The other files of the agent go, also the files that hold no mail of the
   account and the mail of other accounts in them. The next session of
   the job builds them again from its grant. Step 0 runs this build for
   each row of `removed_workspace` of the account whose workspace folder
   exists. If the folder does not exist, step 0 runs only the clear of
   item 2, under the build lock. A build that runs holds the build lock, so this build waits for
   its end. A job with a live session (`reaped_at IS NULL`) is skipped: step
   2 refused each live session of the job at the removal, so this session
   started after the removal. Its build saw the row and already deleted the
   files, and a build must not change the folder of a live session. No
   process that started before the removal can write to the folder after
   this step. A row goes only after the purge of its job: step 0 deletes
   the row after its build, and the build of a session launch deletes the
   row after its own purge. A session that registered after the removal
   but did not build yet keeps its row, so the account stays `removing`
   until that build (step 7). A crash before a purge keeps the row, so the
   next delete builds again.

1. **Items**: each item of the account, and each item that cites a message
   of the account. First the rows that refer to them with no cascade:
   `correction`, and `item.duplicate_of` (set to NULL). The cascades delete
   `item_source`, `matter_item`, `matter_add` (a pending addition of
   migration 46) and the rows that hang on them.
2. **Messages**: for each message: `header`, `body` (the trigger deletes the
   search text), `message_label`, `attention_due`, `link_check`,
   `sender_evidence`, `reply_draft`, `use_open`, and the `notice` rows of the
   message. Then the message row. Each raw message file that no message of
   another account uses is deleted, with the order of `clean_gone`: the file
   goes while the transaction holds the write lock.
3. **Threads and labels**: each thread that no draft refers to, with
   `talk_due` and `nudge_try`. Each label. A thread that a draft refers to
   stays as a bare row (an ID and a provider ID, no mail).
4. **Sync rows**: `sync_log` and `sync_seen` of the account.
5. **Senders**: a sender kind comes from the evidence in the store
   (spec 1 section 6.7): the mail from the address (`header.from_addr`) and
   the evidence rows (`sender_evidence`, for example a recipient of sent
   mail). The delete removes the evidence of the account, so it does two
   things, in the transaction of the last batch:
   1. Delete each `sender` row whose address the mail of the account was
      evidence for, that no message that stays is evidence for (no
      `from_addr` and no `sender_evidence` row), and that the person did
      not decide (`decided_by IS NOT 'person'`). A decision of the person
      about an address is a rule of the person, not mail. Each batch writes
      these addresses (the `from_addr` and each evidence address of its
      messages) to the new table `removed_sender(account_id, address)`
      before it deletes the messages, so this step finds them also after a
      crash. The step deletes the rows of `removed_sender` of the account.
   2. Derive each engine kind again from the evidence that stays in all
      accounts: `iris_senders::refresh`, then
      `iris_understand::store::rerun_senders` for a sender whose `hostile`
      kind changed, as the understanding batch does. So a person known
      through the sent mail of another account stays a person, and a person
      known only through the removed account becomes what the evidence that
      stays says.

6. **Orphan files**: each raw message file that no message row refers to
   and whose time of change is before the start of this delete. A sync of
   the account can write a raw file and then fail at the insert of its row
   (the trigger). The delete holds the sync lock, so such a sync ended
   before this step. A young file of a sync of another account stays. If an
   old file of another account goes in the moment before its row insert,
   that insert finds the file missing and fails with `MissingContent`, as
   with `clean_gone`, and the next sync fetches the message again.
7. **End**: `INSERT INTO message_fts(message_fts) VALUES ('optimize')`.
   Commit. Then `PRAGMA wal_checkpoint(TRUNCATE)`. Only if the checkpoint
   is not busy and no `removed_workspace` row of the account is left, set
   `state = 'removed'`. The waiting call of the command and the API
   tries the checkpoint again each 100 ms, for at most 10 seconds (a reader
   of the engine holds a read transaction for less). The tick tries once. A
   checkpoint that stays busy keeps the state `removing`, so the next call
   does step 7 and the checkpoint again. The write of the
   state adds only the page of the account row to the log.

The engine calls `delete_removed` on each tick, for each account in state
`removing`, and skips a held lock. The command and the API call it once at
once after the removal, for their own account only (`only = Some(id)`): the
call waits for the locks of that account, and its counts are of that
account. An account of an earlier removal that waits for its delete does
not hold up a new removal. So the delete continues after a crash or
a stop, with no new step by the person.

### What stays

The record of what Iris and the person did stays: changesets and their
actions, comments, the ledger, drafts, jobs, job events, job notes, the
job scope rows and the matters that the person decided (`confirmed` or
`declined`). These tables keep each row by design (their delete triggers
refuse). They hold IDs of messages and threads that no longer exist, and text
that the person or an agent wrote: a draft, a reason, a note. A decided matter keeps
its title, which the model wrote from the mail, and loses the members of the
account (the cascade of `matter_item`). A proposed matter goes when one of
its members goes (the trigger `matter_item_lost`), also when other members
are items of another account. A proposal is a suggestion of the engine,
not mail and not a decision of the person: the items of the other account
stay, and a later run of the proposal step can propose a matter from them.
A reader of
these rows must accept an ID with no row (the build checks the ledger view,
the review view and the job page).

The address of the tombstone stays in the account table until the person
adds the address again. The conversation
rule (`iris-talk`) reads each account address as an address of the owner,
so mail to that address from another account still counts as mail to the
owner.

### Add the address again

`save_account_secret` with the address of a tombstone (`removed`) makes a
new account row with a new ID, in one transaction. First it changes the
address of the tombstone to `removed:<id>:<address>`, because
`account.address` is unique. This text is not a mail address, so it never
matches a real address. The tombstone keeps its ID and provider, so each
ledger entry and draft of the old account still names the old mailbox, and
no action of the old account can apply to the new one. The new account has
no sync state, so its first sync is a full copy. An address in state
`removing` is refused: "Iris still deletes the mail of this account. Try
again in a minute."

### Command

```
iris account remove <address> [--yes]
```

With no `--yes`, the command prints what it removes and asks for the address
again on the terminal. It refuses a wrong answer. Then it does the removal
and the delete, and prints the counts and the warning: backups and the
notes of jobs keep their copy. The question of the command and the button
say the same before the removal: the files of the agents in the workspaces
of jobs with mail of the account go.

### The call waits for the delete

The command and the API call do the removal, then the delete of that
account, and return only when the delete ended. For this call the delete
waits for the locks: the sync lock (a sync of the account fails at its next
insert, so it ends soon) and the backup lock (a backup ends by itself). The
engine tick uses no wait: it skips an account whose lock is held.

The result of the call is the state of the account at its end. Only
`removed` is a finished removal: the command prints "removed ... and its
mail", and the client shows the account as gone. With `removing` (a
checkpoint that stayed busy, or a crash), the command prints that the
engine ends the delete, and the Accounts page shows "Iris deletes the mail
of this account" until the state is `removed`.

So when the person sees "removed", no view shows the mail of the account.
During the call, the brief already has no item of the account, but another
view (search, a message by ID) can still show its mail until its batch
goes. After a crash during the call, the state stays `removing`, the
Accounts page says "Iris deletes the mail of this account", and the next
engine tick ends the delete.

### API and web client

- `POST /api/accounts/{id}/remove` with the JSON body
  `{"address": "<address>"}`. The address must match the account: this makes
  a removal by a wrong ID impossible. The removal checks the ID and the
  address in its own write transaction (`remove_account_id`): if another
  request removed the ID and the person added the address again (a new
  ID), the removal is refused with 409 and the new account stays. The response comes after the delete
  (see "The call waits for the delete").
- `/api/status` lists accounts in state `removing` with that state, and does
  not list `removed` accounts.
- The Accounts page gets a "Remove" control for each account, with a confirm
  step that names the address and says what goes and what stays. The layout
  is the owner's pick (before/after page).

## Options not chosen

| Option | Why not |
|---|---|
| Delete the account row | The ledger keeps its entries and refers to the row. A free ID can go to a new account. |
| A soft hide (a flag that every reader filters) | The mail stays on disk. D34 says remove. |
| Delete or blank the record of actions | Its delete triggers refuse by design: the record proves what Iris did. A change of these rules is a larger decision. |
| Delete the backups | A backup is the person's own copy and the only way back after a mistake. |
| One transaction for the full delete | A large mailbox holds the write lock for minutes and stops the engine. |

## Failure list

| Input or event | Result | Test |
|---|---|---|
| Unknown address or ID | Refused, nothing changes. | store test |
| Address of a `removing` or `removed` account | Refused, nothing changes. | store test |
| API body address that does not match the ID | HTTP 400, nothing changes. | API test |
| A job session with scope in the account is live (not reaped) | Refused with "stop the job, then try again". This includes an ended session whose process did not stop yet. | store test |
| A sender row of another account only, with no header | It stays. | store test |
| An address known through sent mail of the account and of another account | It stays a `person`: the refresh reads the evidence of the other account. | API test |
| An address known only through sent mail of the account | Its row goes. | API test |
| An IMAP account | The tombstone keeps no IMAP server, port or user. | store test of each column |
| A sync of the account runs at the removal | Its next insert fails on the trigger. The delete waits for the sync lock. No message of the account stays. | store test with an insert after the removal |
| That sync wrote a raw file before its insert failed | Step 6 deletes the file. | store test with a file and no row |
| An understanding run writes after the removal | The trigger on `item` or `body` refuses the write. The batch query leaves the account out. | store test |
| A workspace build runs at the delete | Step 0 waits for the build lock, then builds again with no mail of the account. | workspace test |
| An agent copied mail of the account into a draft or another file | The build of step 0 deletes each entry other than `notes.md` first. | workspace test |
| `notes.md` is too large, a link or a folder, so the save fails | `notes.md` stays as it is. The other entries go. | workspace test |
| A job has scope in two accounts | The build of step 0 builds the mail of the other account again. The files of the agent go. | workspace test |
| A proposed matter has members in two accounts | The proposal goes (trigger `matter_item_lost`). The items of the other account stay. | store test |
| A crash after the removal, after the items went | `removed_workspace` still names the jobs. | store test |
| A reader keeps the log busy at the checkpoint | The waiting call tries for 10 seconds. If it stays busy, the state stays `removing`, the result says so, and the next call checkpoints again. | store test with an open read transaction |
| An apply step of the account runs at the removal | It holds the sync lock, so the delete waits. Its ledger entry stays. An unknown result of that step stays unknown (no recovery runs for a removed account). | limit issue |
| A crash after the removal, before the delete | State `removing`. The brief is empty of the account. The next tick deletes. | store test: removal, no delete, open again, delete |
| A crash in a batch of the delete | The transaction rolls back. A file deleted before the rollback is missing for a row that the next batch deletes. The next tick continues. | store test: delete in two calls |
| A raw message file that another account uses | The file stays. | store test with one file in two accounts |
| A backup runs | The delete waits for the backup lock. | as `clean_gone` |
| Two deletes at the same time (engine and command) | The sync lock lets one run. The other does nothing. | store test |
| Add the address while `removing` | Refused. | store test |
| Add the address after `removed` | A new row with a new ID, a first sync, zero differences. The tombstone keeps the old ID. | engine test |
| An action of the old account in review, approved after the address comes back | It applies nothing: no sign-in has the old ID, and its targets are gone. | store test (new ID) |
| The call waits for a held sync lock | It returns after the lock goes and the delete ends. | store test with a thread |

## Impact

- `crates/iris-store`: migration 47 (insert triggers, `removed_workspace`,
  `removed_sender`),
  `remove_account`, `delete_removed`, the change of `save_account_secret`.
- `crates/iris-understand`: the batch query leaves out a removed account.
- `crates/iris-workspace`: the grant leaves out a removed account. A build
  of a job with a row in `removed_workspace` deletes each entry other than
  `notes.md` first.
- `crates/iris`: `iris account remove`, the engine tick.
- `crates/iris-api`: the route, the status filter, and `delete_removed` (the
  delete with the workspace build and the sender refresh) for the engine,
  the command and the route.
- `client/`: the Accounts page.
- Memory: no new cache. The delete reads 500 rows at a time.

## Assumptions

| Assumption | Evidence |
|---|---|
| Each table with mail of an account has a path from `account`, `message`, `thread`, `label` or `item`. | The table list of migration 45 (`PRAGMA foreign_key_list` of each table, checked 2026-11-24). The build adds a store test that fails if a new table has a column `message_id`, `thread_id` or `item_id` that the delete does not list. |
| A cascade delete runs the delete triggers of the child table. | SQLite documentation, "Foreign Key Actions". The store test checks the search text. |
| `PRAGMA secure_delete` writes zeros over freed pages. | SQLite documentation, "PRAGMA secure_delete". |
| The readers of the record of actions accept a missing message, thread or item. | `pending`. The build checks the ledger view, the review view and the job page with a removed account. A reader that fails gets a fix in the code PR. |

## External writes

None. The removal and the delete change only the data folder.

## Open questions

- The batch size (500, as `clean_gone`). The build can change it.

## Acceptance criteria

- After `iris account remove`, no row of `message`, `header`, `body`,
  `message_fts`, `thread` (other than a thread of a draft), `label` or `item`
  has the account, and no raw message file of the account exists. Red if one
  table keeps a row.
- The other account keeps all its mail (verify: zero differences). Red if
  the delete touches the other account.
- After the delete, no file of a workspace of the account holds text of
  its mail, other than `notes.md`. Red if a draft of the agent keeps it.
- The ledger and the changesets keep each row. Red if a count goes down.
- A removal with a crash before the delete ends with the same state after the
  next tick.
- The same address added again syncs with zero differences.

## Verify

`cargo test -p iris-store --test remove` and
`cargo test -p iris --test engine remove`.

## Rollout

The migration runs at the next start. No setting.

## Risks

- A removal of the wrong account. The command asks for the address again,
  and the API needs the address in the body. There is no undo: the mail
  comes back only with a new sync from the provider.
- A reader of the record of actions that fails on a missing row. The build
  checks each reader (see Assumptions).

## Plan

1. P11-a store and engine PR: migration, removal, delete, command, tick.
2. P11-a client PR: the API route and the Accounts page, after the owner's
   pick.
