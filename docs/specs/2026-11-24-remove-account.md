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
   workspace file.
3. Iris writes nothing to the provider. The mailbox at the provider does not
   change.
4. A crash at any point leaves a state that the engine completes, with no
   mail of the account in the brief.
5. The person can add the same address again later. It then gets a new
   first sync.

## Not in scope

- A delete at the provider. Iris never deletes a mailbox.
- Backups. A backup copy that the person made (`iris backup`) and the
  migration copies `iris.db.v<N>.<time>-<hex>.bak` keep the mail of the
  account. The command and the button say so. (Limit issue to file with the
  code PR.)
- The record of actions (see "What stays").

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

### The removal (one write transaction)

1. Refuse if the account does not exist, or its state is `removing` or
   `removed`.
2. Refuse if a session of a job runs (`job_session.ended_at IS NULL`) whose
   scope has a thread or an item of the account. The agent has a copy of the
   mail in its workspace. The person must stop the job first.
3. Set `state = 'removing'`, and set `token`, `history_id`,
   `mirror_ready_at`, `sync_kind`, `sync_start` and `sync_step` to NULL. With
   no token, `sync_accounts` drops the account, so no new sync or apply
   starts.
4. Deny each open action (`proposed`, `in_review`, `approved`, `held`) that
   targets a message, an item or a draft of the account:
   `state = 'denied'`, `decided_by = 'engine'`, reason "the account was
   removed". An action with a ledger entry in state `writing` keeps its
   state, as `iris_changeset::expire` does.
5. Delete the items of the account (step "items" below), so the brief is
   empty of the account at the commit.

A migration adds triggers that refuse an insert into `message`, `thread` and
`label` for an account in state `removing` or `removed`. A sync that started
before the removal and holds a token in memory then fails at its next write.
It cannot add mail after the removal.

### The delete

`Store::delete_removed` runs the delete of each account in state
`removing`. It holds the sync lock of the account (`iris_sync::lock`), so no
sync or apply of the account runs at the same time, in this process or in
another. It holds the backup lock, as `clean_gone` does, so a backup does not
copy a half state. If a lock is held, it does nothing and the next call
tries again.

It runs with `PRAGMA secure_delete = ON`, so SQLite writes zeros over the
deleted content. Steps, each in batches of 500 rows, one write transaction
for each batch:

1. **Items**: each item of the account, and each item that cites a message
   of the account. First the rows that refer to them with no cascade:
   `correction`, and `item.duplicate_of` (set to NULL). The cascades delete
   `item_source`, `matter_item` and the rows that hang on them.
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
5. **Senders**: each `sender` row whose address sent no message that is
   still in the store, and that the person did not decide
   (`decided_by IS NOT 'person'`). A decision of the person about an address
   is a rule of the person, not mail.
6. **Workspaces**: the workspace folder (`workspaces/j-<id>`) of each job
   whose scope has a thread or an item of the account. The next session of
   the job builds the folder again from the store, with no mail of the
   account.
7. **End**: `INSERT INTO message_fts(message_fts) VALUES ('optimize')`, set
   `state = 'removed'`, then `PRAGMA wal_checkpoint(TRUNCATE)`.

The engine calls `delete_removed` on each tick. The command and the API call
it once at once after the removal. So the delete continues after a crash or
a stop, with no new step by the person.

### What stays

The record of what Iris and the person did stays: changesets and their
actions, comments, the ledger, drafts, jobs, job events, job notes and the
job scope rows. These tables keep each row by design (their delete triggers
refuse). They hold IDs of messages and threads that no longer exist, and text
that the person or an agent wrote: a draft, a reason, a note. A reader of
these rows must accept an ID with no row (the build checks the ledger view,
the review view and the job page).

The address of the tombstone stays in the account table. The conversation
rule (`iris-talk`) reads each account address as an address of the owner,
so mail to that address from another account still counts as mail to the
owner.

### Add the address again

`save_account_secret` with the address of a tombstone (`removed`) uses the
same row: it sets the provider, the server fields, the token and the state.
The sync state is empty, so the next sync is a first sync. An address in
state `removing` is refused: "Iris still deletes the mail of this account.
Try again in a minute."

### Command

```
iris account remove <address> [--yes]
```

With no `--yes`, the command prints what it removes and asks for the address
again on the terminal. It refuses a wrong answer. Then it does the removal
and the delete, and prints the counts and the backup warning.

### API and web client

- `POST /api/accounts/{id}/remove` with the JSON body
  `{"address": "<address>"}`. The address must match the account: this makes
  a removal by a wrong ID impossible. The response is the new status. The
  delete then runs on a thread of the engine.
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
| A job session with scope in the account runs | Refused with "stop the job first". | store test |
| A sync of the account runs at the removal | Its next insert fails on the trigger. The delete waits for the sync lock. No message of the account stays. | store test with an insert after the removal |
| An apply step of the account runs at the removal | It holds the sync lock, so the delete waits. Its ledger entry stays. An unknown result of that step stays unknown (no recovery runs for a removed account). | limit issue |
| A crash after the removal, before the delete | State `removing`. The brief is empty of the account. The next tick deletes. | store test: removal, no delete, open again, delete |
| A crash in a batch of the delete | The transaction rolls back. A file deleted before the rollback is missing for a row that the next batch deletes. The next tick continues. | store test: delete in two calls |
| A raw message file that another account uses | The file stays. | store test with one file in two accounts |
| A backup runs | The delete waits for the backup lock. | as `clean_gone` |
| Two deletes at the same time (engine and command) | The sync lock lets one run. The other does nothing. | store test |
| Add the address while `removing` | Refused. | store test |
| Add the address after `removed` | The same row, a first sync, zero differences. | engine test |

## Impact

- `crates/iris-store`: migration (states, insert triggers), `remove_account`,
  `delete_removed`, the change of `save_account_secret`.
- `crates/iris`: `iris account remove`, the engine tick.
- `crates/iris-api`: the route, the status filter.
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
