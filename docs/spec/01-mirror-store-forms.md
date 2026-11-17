# Iris specification 1: mirror, store and forms

- Status: draft, for the approval of the maintainer
- Applies to: release 1 ("See")
- Open model version: 0 (draft)
- Related: [Specification 2](02-changeset-policy-ledger.md), the
  [schemas](schema/), the manifesto and the experience document "Iris: the
  ideal experience"

## 1. Scope

This specification defines three parts of the Iris engine:

1. The mirror: the local copy of one Gmail mailbox, and the sync that keeps it
   exact.
2. The store: the files and the SQLite tables that hold all state.
3. The data format of the five forms (commitment, conversation, record, feed,
   signal) and of the sender axis. This format is version 0 of the open model.

It also defines how the engine turns mail into forms (the understanding step),
and how the first brief reads the forms.

Release 1 writes nothing to the mailbox. Writes, the policy file and the
ledger are in specification 2.

Matters are not in release 1 and not in this specification. Each form
document has a `matter` field that stays empty until release 6.

## 2. Terms

| Term | Meaning |
|---|---|
| Provider | The service that holds the mailbox. Release 1 has one provider: Gmail, through the Gmail API v1. |
| Mirror | The local copy of the mailbox: each message in its raw form, its labels and its thread. |
| History ID | A number from Gmail that marks one point in the change history of a mailbox. |
| Raw message | The bytes of one message as RFC 5322 defines them, as Gmail returns them in `format=raw`. |
| Item | One document of one form. An item has an ID such as `c-311`. |
| Source | The link from an item to the message that it comes from. |
| Understanding | The step that reads a message once and makes or updates items. |
| Correction | A change that a person makes to an item (release 2, specification 2). A rebuild keeps each correction. A correction does not change `derived`. |
| Observe mode | The engine makes items and proposes changes, but applies nothing. Release 1 has only observe mode. |

## 3. Requirements

These requirements come from the manifesto and from the experience document.
Each one has a test or a measure.

| ID | Requirement | How we check it |
|---|---|---|
| M1 | The mirror matches Gmail with zero differences. | `iris mirror verify` reports 0 differences each day for 7 days on a real mailbox (exit criterion of release 1). |
| M2 | Release 1 sends no write request to Gmail. | The OAuth scope is `gmail.readonly`. A test of the provider client fails if a code path calls a write endpoint. |
| M3 | The engine stays inside the memory budget. | 50 MB resident memory with 100,000 messages (`crates/iris/tests/budget.rs`). |
| M4 | The first brief appears in 10 minutes or less. | Time from the connect step to the first brief, with the synthetic mailbox and the fake Gmail server. |
| M5 | Mail text is data. | No text from a message can start an action. The understanding step has no tools and returns only schema-valid JSON. |
| M6 | Each item points to its source mail. | Each item has one or more sources. The schemas require the `sources` field. |
| M7 | The forms can be built again from the mirror. | With a fixed fake model, `iris rebuild` makes the same items from the same mirror, apart from IDs and times. Each pinned item keeps its ID and its corrections. |

## 4. The mirror

### 4.1 What the mirror holds

For each message in the account, the mirror holds:

- The raw message, byte for byte.
- The Gmail message ID and thread ID.
- The set of label IDs. The set includes the system labels (`INBOX`, `SENT`, `SPAM`,
  `TRASH`, `UNREAD`, `STARRED`, `IMPORTANT` and the `CATEGORY_*` labels).
- The Gmail `internalDate`, and the size of the raw message in bytes.

For the account, the mirror holds the list of labels and the history ID of the
last complete sync.

The mirror includes messages in spam and trash (`includeSpamTrash=true`).
The labels tell where each message is. "Hidden is never deleted" needs a full
copy, and the sender axis uses the spam label.

### 4.2 First sync

1. Call `users.getProfile`. Save its `historyId` as `H0` in the sync state
   of the account (below). Do this before the list, so that no change between
   the list and the end of the sync is lost.
2. Call `labels.list`. Save each label.
3. Call `messages.list` with `includeSpamTrash=true` and `maxResults=500`.
   Follow `nextPageToken` to the last page. Save each message ID and thread ID
   as "known, not fetched".
4. For each message that is not fetched, call `messages.get` with
   `format=raw`. Decode the base64url data, write the raw message to the
   content folder (section 5.3). Save the labels and the `internalDate` of
   the response, and the length of the decoded data as the size.
   Fetch by thread: first the thread with the newest message, and each
   message of a thread before the next thread. So understanding can start on
   the last 90 days before the full copy is complete (section 7.1). If Gmail returns HTTP 404
   for a message, Gmail deleted it after step 3. Mark the message as gone
   (section 4.5) and continue.
5. When each known message is fetched, gone or failed (section 4.7), run one
   incremental sync from `H0` (section 4.3). Then mark the account as "mirror
   ready". A failed message stays "known, not fetched" in a ready mirror, until
   verify (section 4.8) fetches it.

At the quota of the assumptions, the engine fetches about 100 raw messages
each minute. So the first brief covers the messages that the engine fetched
before it, and the brief shows the date of the oldest of them. The brief
grows while the first sync continues.

The first sync can stop at any point (crash, network loss, restart). It starts
again from the saved state, with the saved `H0`. It fetches only the messages
that are not fetched.

The sync state of the account is in the columns `sync_kind` (`first`,
`resync` or empty), `sync_start` (`H0` or `H1`) and `sync_step` of the
`account` table. The engine writes
`account.history_id` only in the transaction of the last step of a sync. At
start, the engine continues an unfinished first sync or full resync from its
saved step and start history ID. While a first sync or a full resync runs, no
other sync of the account runs.

### 4.3 Incremental sync

1. Call `history.list` with `startHistoryId` set to the saved history ID, and
   with the history types `messageAdded`, `messageDeleted`, `labelAdded` and
   `labelRemoved`. Follow `nextPageToken` to the last page.
2. Apply each history record in the order that Gmail returns:
   - `messageAdded` for a message that the mirror does not have: fetch the
     message (`format=raw`) and save it. If Gmail returns HTTP 404, Gmail
     deleted the message after the record. Do not save the message, and
     continue with the next record.
   - `messageAdded` for a message that the mirror has: save the label set of
     the record. Do not fetch the message again.
   - `messageDeleted`: mark the message as gone (section 4.5). If the mirror
     does not have the message, ignore the record.
   - `labelAdded` and `labelRemoved`: change the label set of the message.
3. Save the `historyId` of the response as the new history ID. Save it only
   after the last change is saved, so that a crash cannot save a history ID
   without its changes. A crash before this step leaves the old history ID.
   The next sync applies the same records again, and gets the same mirror.

If a `labelAdded` or `labelRemoved` record names a message that the mirror
does not have, fetch the message as for `messageAdded`.

Each incremental sync also calls `labels.list` after `history.list`, because
Gmail makes no history record for a new name or a delete of a label. The engine saves each label of
the answer with its name, and deletes each label row, and its `message_label`
rows, that the answer does not have. A history record that names a label that
`labels.list` does not have changes nothing for that label.

### 4.4 Full resync

Gmail keeps the change history for a limited time. If `history.list` returns
HTTP 404 for the saved history ID, do a full resync:

1. Call `users.getProfile`. Save its `historyId` as `H1` in the sync state
   of the account.
2. Call `labels.list`, and save the labels (section 4.3).
3. List all message IDs and thread IDs again (as in step 3 of section 4.2).
4. Fetch each message that the mirror does not have. If Gmail returns HTTP
   404, Gmail deleted the message after step 3. Do not save the message, and
   continue.
5. For each message that the mirror has and the list does not have, call
   `messages.get` with `format=minimal`. Mark the message as gone only if
   Gmail returns HTTP 404. Else, keep it, and correct its labels from the
   answer.
6. If a message that the mirror marked as gone is in the list, clear its
   `gone_at`. If its content file is deleted, fetch the message again. Then
   run understanding again on the message (section 7.1).
7. Get the label sets (section 4.10). Correct the label set of each message
   that the mirror has and the list has.
8. Run one incremental sync from `H1`. This sync corrects each change that
   Gmail made during steps 2 to 7.

Step 7 does not call `messages.get` for each message, so a full resync costs
about the same quota as a list of all message IDs, plus the fetch of new
messages. The engine records each full resync in the sync log, so that a
person can see why a sync took long.

### 4.5 Messages that are gone

When Gmail deletes a message for good, the mirror must follow. The engine:

1. Sets `gone_at` on the message. The message leaves each query at once.
2. Keeps the raw message for 30 days, so that a sync defect cannot destroy
   mail that is still at Gmail. A full resync in these 30 days can bring the
   message back (section 4.4, step 6).
3. After 30 days, removes the reference of the message to its content file
   (section 5.3), and deletes its `header`, `body` and search text. The
   message row stays as a tombstone (`gone_at` set, `content_sha256` empty)
   while an item cites it in `item_source`. The engine deletes the tombstone
   when no item cites it.

An item keeps each of its sources when a source message is gone. A source
that cites a tombstone is still a valid source, so the origin source of an
item never goes away. The quote of the origin stays in the item document.

When each source of an item is gone, the item gets the status `gone`, and the
engine saves the status before it in the item row. When only some sources are
gone, the item keeps its status. The brief does not show an item with the
status `gone`. If a source message comes back (section 4.4, step 6), the item
gets its saved status again.

### 4.6 When to sync

- The engine runs an incremental sync each 60 seconds, and at once when a
  person opens the brief.
- Each account has its own sync thread and its own quota count, so a long
  first sync of one account does not delay the syncs of another account.
- Gmail push notifications need Google Cloud Pub/Sub and a public endpoint.
  A self-hosted engine often has no public endpoint, so release 1 does not use
  push. A later release can add push as an option.

### 4.7 Quota, errors and limits

- The engine keeps to the Gmail per-user quota (see the assumptions). It counts
  quota units for each call, and it waits when the next call would go over the
  limit.
- The engine runs at most 8 `messages.get` calls at the same time. This number
  is a constant in the provider client.
- These rules are for read calls. Specification 2, section 7.2 gives the
  rules for a write call.
- On HTTP 429, on HTTP 403 with the reason `rateLimitExceeded` or
  `userRateLimitExceeded`, and on HTTP 5xx, the engine waits and tries again.
  The wait starts at 1 second, doubles each time to a maximum of 5 minutes,
  and has a random part of up to 50 percent.
- On HTTP 401, the engine gets a new access token with the refresh token.
  An access token lives about 1 hour, and one call of a first sync can run
  for many hours. A 401 from a token at the end of its life is not a
  refusal: the engine gets a new token and continues. A 401 from a token
  that is not at the end of its life is a refusal. If two tokens are
  refused in one call, or Google refuses the refresh
  token (`invalid_grant`), the account goes to the state "needs sign-in", and
  the brief shows it. A network error or a server error of the token endpoint
  keeps the state: the next sync tries again. A refusal changes the state only
  while the stored sign-in is the one that failed, so a new sign-in in the
  meantime keeps its state.
- A message that gives an error that a new try does not fix (a bad answer,
  or HTTP 400) does not stop the sync. The engine records the message in the
  sync log with the kind `fetch`, keeps it as "known, not fetched", and
  continues. Verify (section 4.8) finds the message and fetches it again.
- The engine never parses a message body in the sync. The sync stores bytes.
  The understanding step reads them later (section 7). A defect in a parser
  cannot stop the mirror.

### 4.8 Verify

`iris mirror verify` compares the mirror with Gmail, and checks the local
content:

1. Run one incremental sync. Then stop the sync of the account until verify
   ends, so that the mirror does not change. Save the history ID of the
   mirror as `V`.
2. Call `labels.list`.
3. List all message IDs and thread IDs at Gmail (as in step 3 of
   section 4.2).
4. Get the label sets (section 4.10).
5. Call `labels.list` again.
6. Call `history.list` from `V`. Each message that a history record names
   changed during verify. Leave these messages out of the compare. A label
   that is not the same in steps 2 and 5 changed during verify. Leave it out
   of the compare. If `history.list` returns HTTP 404, stop verify and report
   that the history is too old.
7. Compare the message IDs, the thread IDs, the label sets and the label list
   with the mirror. Compare only the messages of the mirror that are not gone.
   A message that is not fetched (section 4.7) has no label set yet, so its
   labels are not compared. Step 8 fetches it. A list can miss a message
   when a delete moves the list pages. For each mirror message that the list
   does not have, call `messages.get` (`format=raw`). Only HTTP 404 makes it
   a difference. Otherwise, compare its thread ID and its labels from that
   answer. A label list can also miss a message when a delete moves its
   pages. Before you report a label difference, get the labels of the
   message (`format=minimal`). Report it only if those labels are not the
   labels of the mirror.
8. For each message that is not gone, read its content file and compute the
   SHA-256 hash. A missing file, or a hash that is not `content_sha256`, is a
   difference. Fetch the message again to repair the file. If a fetch gets
   an error that stops each fetch (for example HTTP 401), start no more
   fetches. Save the result, then give the error to the caller.
9. Print each difference, and the total. Save the result in the sync log.

If an error ends the compare, save the differences found before the error,
and start a full resync if one of them is a difference of step 7.

However verify ends (a result, an error or a crash), the engine starts the
sync of the account again. After a crash, the next start does this. A
difference of step 7 starts a full resync (section 4.4).

Verify does not compare `internalDate` or the size. Gmail does not change the
bytes of a message, and step 8 checks the bytes. A difference of step 8 is a
local defect, not a sync defect. The engine fetches the message again
(`format=raw`) to repair it.

The exit criterion of release 1 is 0 differences each day for 7 days.

### 4.9 Account and credentials

- The person signs in with OAuth 2.0 in the browser. The engine uses the
  loopback redirect (`http://127.0.0.1:<port>/oauth/callback`) and PKCE.
- Release 1 asks only for the scope `https://www.googleapis.com/auth/gmail.readonly`.
  Release 2 asks for `gmail.modify` when the person turns on writes.
- The engine stores the refresh token encrypted in the store (section 5.4),
  together with the OAuth client file that got it, in `account.token`. So each
  account has its own client, and a backup has a matching client and token.
  The engine keeps the access token only in memory.
- The OAuth client ID is a setting. Each user gives the JSON file of an own
  OAuth client of the type "Desktop app", published to "In production".
  Do not use the "Testing" status: its refresh tokens expire after 7 days.
  See `docs/decisions/2026-11-google-sign-in.md` (tracker item R1-01).
- An agent never gets a token. Only the provider client in the engine reads
  the token.

### 4.10 Label sets

The full resync and verify get the label set of each message from lists, not
from one `messages.get` call for each message:

1. For each label of `labels.list`, call `messages.list` with `labelIds` set
   to that label and `includeSpamTrash=true`. Follow `nextPageToken` to the
   last page.
2. The label set of a message is the set of labels whose list names the
   message. A message that no list names has no labels.

A list page costs 5 quota units and names up to 500 messages. One
`messages.get` call costs 20 units for one message.

## 5. The store

### 5.1 Data folder

All state of one Iris install is in one data folder:

| System | Default path |
|---|---|
| macOS | `~/Library/Application Support/iris` |
| Linux | `$XDG_DATA_HOME/iris`, or `~/.local/share/iris` |

The setting `--data <path>` or the variable `IRIS_DATA` changes the path. The
engine makes the folder with mode `0700`.

```
iris/
  iris.db            SQLite database: all state except raw messages
  iris.db-wal        SQLite write-ahead log
  key                encryption key for secrets, mode 0600
  content/           raw messages, one file each
    3f/
      3fa1...c9.eml
  policy.toml        the policy file (specification 2)
  iris.sock          the socket of the iris command (specification 2)
  workspaces/        the workspace of each agent job (docs/specs/2026-11-16-job-workspace.md)
```

To back up Iris, stop the engine and copy the folder. `iris backup <path>`
makes a copy while the engine runs:

1. Take the backup lock. While a backup holds this lock, the content cleanup
   (section 4.5) does not start, and the engine does not write
   `policy.toml`. The cleanup takes the same lock for its full step: the
   database change and the file delete.
2. Copy `iris.db` with `VACUUM INTO`. It reads one snapshot in one read
   transaction. The SQLite online backup starts again after each write of
   another connection, so it can fail to end while the engine writes.
3. Copy each content file that the copy of `iris.db` refers to.
4. Copy `key` with mode `0600`, and copy `policy.toml`. Without `key`, the
   backup cannot decrypt the secrets.
5. Release the lock.

The engine makes the backup folder with mode `0700`, because it holds the key
and the mail. The engine writes `policy.toml` to a temporary file, and it
does not write the file during a backup. So the copy has one complete version
of the policy. The person can save the file at any time, so the engine first
renames the old file to a copy, checks that copy, and then puts the new file
in place with a hard link, which never replaces a file. The engine never
deletes the old file: it goes back, or it stays as the copy, because an
editor can hold it open and save into it later. So a save during the write
is never lost.

New content files after step 2 are not in the backup, and the copy of
`iris.db` does not refer to them.

`iris backup` does not copy `workspaces/`. A copy of the whole folder while
the engine is stopped copies `workspaces/` too, with the files of the agent.
The build makes the files of a
workspace again from the store, before each session. The files of the agent
(`drafts/` and `notes.md`) are not in a copy of `iris backup`: after a
restore from it, the next build makes them empty. A draft that the agent submits is in the store
(specification 2), so only work that the agent did not submit is lost. This
is a known limit, with a GitHub issue, until the notes between sessions
(R5-06) move into the store.

### 5.2 SQLite settings

The engine uses SQLite through `rusqlite` with the `bundled` feature, so FTS5
is always present and each system has the same SQLite version.

| Setting | Value | Reason |
|---|---|---|
| `journal_mode` | `WAL` | Readers do not block the writer. |
| `synchronous` | `FULL` | Each commit is on disk before the next step. The ledger row of specification 2 must be on disk before the provider write, or a power loss can lose the record of a write. |
| `foreign_keys` | `ON` | The schema uses foreign keys. |
| `cache_size` | `-2048` (2 MiB) for each connection | Keeps the memory budget. |
| `mmap_size` | `0` | Memory-mapped pages count in resident memory on some systems. |
| `temp_store` | `FILE` | Large sorts use disk, not memory. |
| `busy_timeout` | `5000` | A reader waits for a checkpoint. |

The engine has one write connection and at most 2 read connections. All writes
go through the write connection.

### 5.3 Content folder

- The engine writes each raw message to `content/<aa>/<sha256>.eml`. `<sha256>`
  is the SHA-256 hash of the raw bytes, in lowercase hexadecimal, and `<aa>` is
  its first two characters.
- The engine writes to a temporary file in the same folder, calls `fsync`,
  and then renames the file. A crash cannot leave a half file under a final
  name.
- Two messages with the same bytes share one file. The database counts the
  references. The engine deletes a file only when no message refers to it.
- The files are plain `.eml` files. A person can open each file with a mail
  program. This keeps the promise "you can leave".

`shortcut:` release 1 stores raw messages without compression. Add compression
if the content folder is too large on real mailboxes.

### 5.4 Secrets

- The engine encrypts each secret (refresh tokens, a model API key) with
  XChaCha20-Poly1305 before it writes the secret to `iris.db`.
- The key is 32 random bytes in the file `key`, mode `0600`. The engine makes
  the key at the first start.
- A copy of `iris.db` alone does not give the secrets. A copy of the full data
  folder does. The documentation must say this.
- The engine never writes a secret to a log, to an error message or to an
  export.

### 5.5 Tables

The engine makes the tables with numbered migrations that are in the program.
`PRAGMA user_version` holds the number of the last migration. Migrations go
forward only. Before a migration, the engine makes a backup copy of `iris.db`.

The table below gives the tables of release 1. Column types are SQLite types.
Times are integers: milliseconds since the Unix epoch, UTC.

| Table | Columns | Purpose |
|---|---|---|
| `account` | `id` INTEGER PK, `provider` TEXT, `address` TEXT UNIQUE, `history_id` TEXT, `state` TEXT, `token` BLOB (encrypted), `mirror_ready_at` INTEGER, `sync_kind` TEXT, `sync_start` TEXT, `sync_step` INTEGER | One row for each mailbox. The `sync_` columns hold the state of an unfinished first sync or full resync (section 4.2). |
| `sync_seen` | `account_id`, `message_id`, `labels` TEXT (JSON), PK(`account_id`, `message_id`) | The messages that the lists of a full resync or a verify name, with their label sets. The engine clears it when a new run starts, not when an unfinished run continues after a crash. |
| `label` | `id` INTEGER PK, `account_id`, `provider_id` TEXT, `name` TEXT, `kind` TEXT (`system` or `user`), UNIQUE(`account_id`, `provider_id`) | Labels of the provider. |
| `thread` | `id` INTEGER PK, `account_id`, `provider_id` TEXT, `viewed_at` INTEGER, UNIQUE(`account_id`, `provider_id`) | Threads of the provider. Gives the `t-` IDs. `viewed_at` is the last view of the thread in Iris. |
| `message` | `id` INTEGER PK, `account_id`, `thread_id`, `provider_id` TEXT, `content_sha256` TEXT, `size` INTEGER, `internal_date` INTEGER, `fetched_at` INTEGER, `gone_at` INTEGER, `understood_at` INTEGER, `hostile` TEXT (JSON), `tries` INTEGER, `retry_at` INTEGER, UNIQUE(`account_id`, `provider_id`) | One row for each message. Gives the `m-` IDs. `hostile` is the list of the hostile marks of the message (section 6.7). `tries` and `retry_at` hold the failed tries of the model (section 7.4). The store clears `understood_at` when the content hash changes, when a gone message comes back, and when the message gets or loses the label `SENT` or `DRAFT`. |
| `message_label` | `message_id`, `label_id`, PK(`message_id`, `label_id`) | The label set of each message. |
| `header` | `message_id` PK, `date` INTEGER, `from_addr` TEXT, `from_name` TEXT, `to_addrs` TEXT, `cc_addrs` TEXT, `subject` TEXT, `message_id_hdr` TEXT, `in_reply_to` TEXT, `list_id` TEXT, `auth` TEXT | Parsed headers that queries need. `auth` holds the SPF, DKIM and DMARC results from `Authentication-Results`. `to_addrs` and `cc_addrs` hold bare lowercase addresses joined by `,`. |
| `body` | `message_id` INTEGER PK, `subject` TEXT, `sender` TEXT, `text` TEXT | The clean text of the message (section 7.2), with the subject and the sender name and address. `message_id` is the row ID. |
| `message_fts` | FTS5 table on `subject`, `sender`, `text`, with `content='body'` and `content_rowid='message_id'` | Full-text search. Triggers on `body` keep it current. |
| `sender` | `id` INTEGER PK, `address` TEXT UNIQUE, `name` TEXT, `kind` TEXT, `basis` TEXT, `decided_by` TEXT, `decided_at` INTEGER, `declined_at` INTEGER | The sender axis. Gives the `p-` IDs. `declined_at`: the time of a decline at the stranger gate (section 6.7), or NULL. |
| `sender_evidence` | `message_id` FK, `address` TEXT, `rule` TEXT (`sent`, `replied` or `service`), `content_sha256` TEXT; PK (`address`, `rule`, `message_id`) | The messages that prove a `person` or `service` kind (section 6.7). The sender step writes the rows. A `sent` row: the sent message names the address in `To`, `Cc` or `Bcc`. A `replied` row: the sent message comes after a message from the address in its thread. A `service` row: the message from the address is automated mail with a Gmail DKIM pass; the rule counts its 2 earlier messages when it reads the row. A row counts only while its message is not gone and has the same content hash. |
| `item` | `id` INTEGER PK, `form` TEXT, `account_id`, `status` TEXT, `status_before_gone` TEXT, `doc` TEXT (JSON), `created_at`, `updated_at`, `derived_by` TEXT, `duplicate_of` INTEGER | One row for each item of each form. `doc` is the form document of section 6. `duplicate_of` marks a possible duplicate (section 7.5). |
| `item_source` | `item_id`, `message_id`, `role` TEXT, PK(`item_id`, `message_id`) | Many to many: one message can feed more than one item. |
| `correction` | `id` INTEGER PK, `item_id`, `field` TEXT, `value` TEXT (JSON), `by` TEXT, `at` INTEGER | Each change of an item by a person, or by an applied action of specification 2. `by` is `person` or the changeset ID. The item stays through a rebuild (section 7.5). |
| `sync_log` | `id` INTEGER PK, `account_id`, `kind` TEXT, `started_at`, `ended_at`, `result` TEXT | Each sync, resync and verify. |
| `understand_run` | `id` INTEGER PK, `started_at`, `ended_at`, `model` TEXT, `prompt` TEXT, `messages`, `calls`, `input_tokens`, `output_tokens`, `cost_micro_usd`, `unknown_calls`, `unknown_cost_micro_usd`, `reserved_micro_usd`, `stop` TEXT | Each run of understanding, with its calls, tokens and cost (`docs/specs/2026-11-16-cost-control.md`). |
| `understand_estimate` | `code` TEXT PK, `model_version`, `messages`, `input_tokens`, `output_tokens`, `cost_micro_usd`, `created_at` | One-time estimate receipts for the budget approval (`docs/specs/2026-11-16-cost-control.md`). |
| `matter`, `matter_item`, `matter_run` | See `docs/specs/2026-11-16-matters.md` (migration 16). | Matters: the proposals of Iris, the decisions of the person, and the last proposal call. |
| `model_setting` | `id` INTEGER PK (one row), `version` INTEGER, `provider` TEXT, `base_url` TEXT, `model` TEXT, `price_in`, `price_out`, `api_key` BLOB (encrypted), `max_tokens`, `timeout_s`, `budget_micro_usd` | The model of understanding, and the budget that the person approved for paid runs (`docs/specs/2026-11-16-cost-control.md`). |

The `message`, `thread`, `sender` and `item` tables use `INTEGER PRIMARY KEY
AUTOINCREMENT`, so SQLite never gives the row ID of a deleted row to a new
row (section 5.7).

The engine adds indexes for the queries of the brief: `item(form, status)`,
`message(thread_id, internal_date)` and `header(from_addr)`.

### 5.6 Full-text search

- The FTS5 table uses the tokenizer `unicode61 remove_diacritics 2` and the
  prefix index `prefix='2 3'`.
- The FTS5 table uses external content, so the text is stored one time, in
  `body`.
- `iris search` accepts words, phrases in quotes, and the filters `from:`,
  `to:`, `after:`, `before:`, `label:` and `form:`. The engine turns the
  filters into SQL conditions and the words into an FTS5 query. It never
  passes the text of a person or an agent to FTS5 without the escape of
  quotes and operators.
- The sync stores bytes only (section 4.7). The `header` and `body` rows,
  and so the words and the `from:` and `to:` filters, come from step 8 of
  understanding (section 7.1). Until understanding reads a message, only
  `label:`, `after:` and `before:` find it. The engine runs understanding
  after each sync (tracker item R1-28, after R1-10).

### 5.7 IDs

Iris gives short IDs that a person and an agent can type:

| Prefix | Object |
|---|---|
| `m-` | Message |
| `t-` | Thread |
| `p-` | Sender (person or service) |
| `c-` | Commitment |
| `v-` | Conversation |
| `r-` | Record |
| `f-` | Feed item |
| `s-` | Signal |
| `g-` | Matter (`docs/specs/2026-11-16-matters.md`) |
| `w-` | Watch (card R5-07) |
| `n-` | Notice by rule (card R5-08) |

The number after the prefix is the row ID of the table. An ID is unique in one
Iris install and the engine never uses it again, also after a rebuild deletes
the row (section 5.5). Provider IDs stay in the
store and do not go into the form documents, so the format does not depend on
Gmail.

## 6. The forms: data format

This section is version 0 of the open model. The schemas in
[`schema/`](schema/) are the normative copy. If this text and a schema do not
agree, the schema is correct and this text has a defect.

All schemas use JSON Schema draft 2020-12.

### 6.1 Common fields

Each item of each form has these fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `model` | string, `"iris/0"` | yes | The version of the open model. |
| `id` | string, for example `"c-311"` | yes | The ID of the item (section 5.7). |
| `form` | `commitment`, `conversation`, `record`, `feed` or `signal` | yes | The form. |
| `status` | string, values for each form | yes | The state of the item. |
| `title` | string, 1 to 200 characters | yes | One line for the brief. |
| `sources` | array of source, at least 1, with exactly one `origin` | yes | The messages that the item comes from. The `origin` source is the message that made the item. |
| `created` | date-time (RFC 3339) | yes | When the engine made the item. |
| `updated` | date-time | yes | The last change. |
| `derived` | object | yes | Who made the item: `by` is `rule`, `model` or `person`. `rule` names the rule. `model` names the model, and `prompt` names the prompt version. |
| `confidence` | number from 0 to 1 | no | How sure the understanding step is. Not set when `derived.by` is `person`. |
| `matter` | string or null | no | The `g-` ID of the confirmed matter of the item, or null (`docs/specs/2026-11-16-matters.md`). The engine sets it when it reads the item. |
| `attention` | object | no | Reserved for specification 2: `state` is `shown`, `bundled` or `hidden`, with the rule and the reason. |

A source has these fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `message` | string, `m-` ID | yes | The message. |
| `thread` | string, `t-` ID | yes | Its thread. |
| `role` | `origin`, `update` or `evidence` | yes | `origin` made the item. `update` changed it. `evidence` supports it. |
| `quote` | string, 1 to 500 characters | when `role` is `origin` | The words in the message that support the item. |

A party (a person or a service in an item) has these fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `kind` | `self` or `sender` | yes | `self` is the owner of the mailbox. |
| `sender` | string, `p-` ID | when `kind` is `sender` | The sender row. |
| `name` | string | no | The display name. |
| `address` | string (email) | no | The address. |

### 6.2 Commitment

An ask or a promise with an owner and a date.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `direction` | `you_owe` or `owed_to_you` | yes | Who must act. |
| `owner` | party | yes | The party that must act. |
| `counterparty` | party | yes | The party that waits. |
| `kind` | `task`, `reply`, `payment`, `invitation` or `decision` | yes | What the act is. The client chooses verbs from it. |
| `due` | object | no | `date` (date) or `at` (date-time), and `strict` (true if the date is firm). |
| `amount` | money | when `kind` is `payment` | The amount to pay. |
| `status` | `open`, `done`, `declined`, `deferred`, `cancelled` or `gone` | yes | |
| `deferred_until` | date | when `status` is `deferred` | |

The understanding step makes a commitment only when the mail has a clear ask
or a clear promise. "Let us catch up some time" is not a commitment.

### 6.3 Conversation

The talk with one person or one group.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `thread` | string, `t-` ID | yes | The thread. One conversation for each thread in version 0. |
| `participants` | array of party, at least 1 | yes | Each party in the thread. |
| `role` | `direct`, `group` or `cc` | yes | `cc` when you are only in CC. |
| `latest` | object | yes | `at` (date-time), `from` (party) and `snippet` (up to 200 characters). |
| `messages` | integer | yes | The number of messages that the conversation counts (section 7.3). |
| `new` | integer | yes | Messages after your last view in Iris. This is not an unread count of the mailbox. |
| `your_turn` | boolean | yes | True when the last message is from another party and has your address in `To`. |
| `summary` | string, up to 1,000 characters | no | A short summary of the thread. |
| `status` | `active`, `quiet`, `muted` or `gone` | yes | `quiet`: no message for 14 days. |

### 6.4 Record

A fact that you keep and can query.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `type` | `receipt`, `booking`, `statement`, `ticket`, `contract`, `event` or `other` | yes | The type of record. |
| `issuer` | party | yes | The party that sent the record. |
| `fields` | object | yes | The fields of the type (table below). |
| `status` | `kept`, `corrected` or `gone` | yes | `corrected`: a person changed a field. |

| Type | Fields (required in bold) |
|---|---|
| `receipt` | **`vendor`**, **`amount`**, **`date`**, `order`, `items` |
| `booking` | **`kind`** (`flight`, `hotel`, `train`, `car`, `restaurant`, `other`), **`reference`**, **`start`**, `end`, `location` |
| `statement` | **`issuer_name`**, **`period_start`**, **`period_end`**, `balance`, `due_date` |
| `ticket` | **`event`**, **`start`**, `venue`, `seat` |
| `contract` | **`name`**, `parties`, `effective`, `expires` |
| `event` | **`name`**, **`start`**, `end`, `location` |
| `other` | **`summary`** |

Money is an object: `value` is a decimal number as a string (`"312.77"`), and
`currency` is an ISO 4217 code (`"USD"`). A string keeps the exact value,
because a binary floating-point number cannot hold each decimal value.

An invoice gives a `receipt` record or a `statement` record, and a commitment
of kind `payment`. The two items share the same source.

### 6.5 Feed

An item to read when you choose.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `publisher` | party | yes | The sender of the feed. |
| `list` | string | no | The `List-Id` header, when the message has one. |
| `published` | date-time | yes | The date of the message. |
| `summary` | string, up to 500 characters | no | |
| `status` | `new`, `read`, `skipped` or `gone` | yes | State in Iris. It does not change the mailbox in release 1. |

### 6.6 Signal

A short-lived item that matters at one moment.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `kind` | `code`, `security`, `delivery`, `build` or `other` | yes | |
| `issuer` | party | yes | |
| `code` | string, up to 32 characters | no | A login code. Only for `kind` `code`. |
| `expires` | date-time | yes | When the signal stops to matter. Default: 15 minutes after the message for a code, 7 days for the other kinds. |
| `status` | `active`, `used`, `dismissed`, `expired` or `gone` | yes | |

The engine sets `status` to `expired` after `expires`. The client never shows
an expired code in large type.

### 6.7 The sender axis

Each sender row has a kind after the sender step decides it (see the
paragraph about a row with no kind below). The kind is a property of the
address, not of one message.

| Kind | Rule in version 0 |
|---|---|
| `person` (known person) | You sent at least one message to the address, or you replied in a thread with it. |
| `service` (known service) | The mail is automated (`List-Unsubscribe`, `List-Id`, `Precedence: bulk`, or `Auto-Submitted` other than `no`), DKIM passes for the domain of the address, and at least 2 messages with an earlier `internalDate` came from it. |
| `hostile` | The domain of the address is a near copy of the domain of a known sender, or a person set the kind `hostile`. |
| `stranger` | Each other sender. |

Order: the engine checks `hostile` first, then `person`, then `service`. A
sender that matches no rule is a `stranger`.

A sender row can have no kind yet. A step that needs the `p-` ID of an
address (conversations, people) adds the row with the address and the name
only, and never writes a kind. The sender step (section 7.1, step 3) sets the
kind, `basis` and `decided_by`. A row with no kind is not decided, and no
step treats it as `person` for a treatment of this section. The people
profiles are an exception for display only: a profile shows an address
with no kind that matches the `person` rule, and the near-copy check of
`hostile` waits for the sender step. When the sender step sets a kind other
than `person`, the profile drops the address.

`basis` records the rule that matched. `decided_by` is `engine` or `person`.
A decision by a person always wins over the engine, and the engine never
changes it.

#### The stranger gate

From release 5, the brief shows the items of strangers as a gate, by sender.
The person gives one answer for each sender:

| Answer | Effect |
|---|---|
| Accept | The kind becomes `person`, with `decided_by` `person` and the `basis` "accepted at the gate". The items go to their sections ("You owe", "Owed to you", "Conversations"). |
| Decline | The engine sets `declined_at` on the sender row. The kind stays with the engine, so a declined sender can still become `hostile`, or known (for example after a reply of the person). The decline applies only while the kind is `stranger`: the items of a declined stranger are in no section of the brief, and the web client lists the declined senders. |
| Reopen | Removes the accept or the decline of the gate. After an accept, the engine decides the kind again from the evidence in the store. A kind that a person set for another reason (for example, a correction) stays. |

The web client shows the gate in the brief: one row for each sender, with
the newest 3 items, "Ignore" and "Accept". After an answer, the row keeps
its place and shows one line with "Undo" (reopen, or decline again after a
reopen from the list of declined senders). The brief shows the first 20
senders and links all of them. The list of declined senders has "Ask again"
(reopen) and "Accept". An API without the gate gives no `gate` in the
brief, and the web client then shows the strangers section as a list of
items, as in release 1.

A rule decides first. A `hide` or a `bundle` rule with `sender_kind =
"stranger"` (specification 2, section 6.2) takes the items of strangers out of
the gate, as it does for each section. A rule never accepts a sender: only the
person makes a stranger a known person. Agents get no answer of the gate.

The engine accepts an answer only for a sender at the gate: the stored kind is
`stranger` (or no kind yet), and, unless a person set the kind, the rules of
this section give `stranger` for the mail of now. The stored kind can be older
than the evidence until the next refresh, and a person must never accept a
sender that is `hostile` now. Also, an item of the sender must wait at the
gate in the same transaction (for an accept, an item on the list of declined
senders is enough), because a rule can take the items out after the person
saw them. A repeated answer is no change. Else the engine refuses the answer
and changes nothing.

An accept makes the domain of the sender known, and a reopen of an accept can
make it unknown. So in the same transaction the engine decides again each
sender whose domain is a near copy of that domain. For each sender that
becomes `hostile` or stops being `hostile`, the same transaction deletes the
items that understanding made from its mail and marks that mail for
understanding again. The understanding worker runs the model later, out of
the transaction, so the items of such a sender come back after the next
understanding batch.

The engine decides the kind of a sender again each time it understands a
message with that address. After each batch of sync or understanding, a
refresh derives each stored `person`, `service` and `hostile` kind again
from the evidence in the store, so a message that goes, gets new bytes or
loses `SENT` changes the kinds at the next refresh. The first sync fetches
newest first, so a known person can be a `stranger` for some time. The brief reads the kind of each
party when it runs its query (section 8), so a later change of a kind moves
the items. If a sender becomes `hostile` or stops being `hostile`, the engine
runs understanding again on each message from that address.

A message with the label `SENT` is from you (`self`), also when its `From` is
an alias. The engine does not understand a message with the label `DRAFT`,
and a draft does not count for the `person` rule.

The engine reads the authentication results only from the
`Authentication-Results` header that Gmail added: the top header with the
authserv-id of Gmail. A sender can add its own header with any result, so the
engine ignores each other `Authentication-Results` header.

An attacker can put the address of a known person in `From`. So the engine
also gives each message hostile marks, separate from the kind of its sender.
The `hostile` column of the message holds each mark that applies:

| Mark | Rule |
|---|---|
| `dmarc` | `Authentication-Results` reports a DMARC fail for the domain of `From`. |
| `spoof` | The display name is the name of a known person, the address is not the address of that person, and DKIM or DMARC does not pass for the domain of the address. If DKIM and DMARC pass, the address is a new sender, with the kind `stranger`. |
| `spam` | The message has the label `SPAM`. |
| `phishing` | The understanding step marks the message as phishing. |

A message with the label `SENT` is your mail. It never gets the marks `dmarc`
and `spoof`, because these marks test the `From` header of mail that you
receive.

The engine computes the marks `dmarc`, `spoof` and `spam` again at each change
of the label set of the message. The mark `phishing` stays until a rebuild.
When a mark is set, or the last mark is cleared, the engine runs understanding
again on the message (section 7.1).

The kind changes the treatment of items (from the manifesto):

| Kind | Treatment in release 1 |
|---|---|
| `person` | An ask becomes a commitment in "You owe" at once. |
| `service` | Records, feed items and signals. Payment commitments go to "You owe". |
| `stranger` | Items go to "Strangers" in the brief, not to "You owe". From release 5 they wait at the stranger gate until the person accepts or declines the sender. |
| `hostile` | The understanding step does not send the body to a model. The engine makes no item. The brief shows a count only. |

A message with a hostile mark gets the treatment of `hostile`, whatever the
kind of its sender. In this specification and in specification 2, "hostile
mail" is a message with a hostile mark, or a message from a `hostile` sender.

### 6.8 Status changes

| Form | Change | Who, and when |
|---|---|---|
| Each form | To `gone`, and back | The engine (section 4.5). |
| Commitment | `open` to `done`, `declined`, `deferred` or `cancelled` | A person, through a changeset (release 2). Understanding sets `done` or `cancelled` when a later message of the thread says so. |
| Commitment | `deferred` to `done`, `declined`, or `deferred` with a new date | A person, through a changeset (release 2): `close`, `decline` and `defer` (specification 2, section 4). |
| Commitment | `deferred` to `open` | The engine, on the `deferred_until` date (UTC). The engine checks once each minute. |
| Conversation | `active` to `quiet`, and back | The engine: 14 days with no message, and the next message. |
| Conversation | `muted` | Reserved. A later release adds the verb. |
| Record | `kept` to `corrected` | A correction (release 2). |
| Feed | `new` to `read` | The engine, when the message loses the label `UNREAD`. |
| Feed | `skipped` | Reserved. A later release adds the verb. |
| Signal | `active` to `expired` | The engine, at `expires`. |
| Signal | `used`, `dismissed` | Reserved. A later release adds the verb. |

## 7. Understanding: from mail to forms

### 7.1 Steps

The engine reads each message one time, after the mirror has it. The steps:

1. Parse the MIME structure and the headers. Save the `header` row.
2. Make the clean text (section 7.2). Save the `body` row and the search text.
3. Decide the sender kind and the hostile mark (section 6.7).
4. Apply the conversation rule (section 7.3).
5. Apply the other fixed rules (section 7.3). If a rule gives the result, go
   to step 8.
6. Send the clean text to the model (section 7.4).
7. If the model answer has `phishing` true, drop the items of steps 5 and 6,
   and give the message the treatment of hostile mail. Compute the
   conversation of the thread again without the message.
8. Validate each item against the schemas. Save the items and set
   `understood_at` on the message in one SQLite transaction. A crash before
   the commit leaves the message not understood, and the next start runs the
   steps again from step 3.

To run understanding again on one message, the engine first deletes the items
whose origin is that message and that are not pinned (section 7.5). Then it
runs the steps from step 3. The conversation of the thread is computed again.

The engine understands mail by thread. It understands a thread when the
mirror has each known message of the thread, and it understands the messages
of a thread oldest first. So a later message that closes a commitment comes
after the commitment. The threads of the last 90 days come first, the thread
with the newest message first. Older mail gets understood in the background,
at a low rate, so that the first brief comes early (requirement M4). A
rebuild uses the same order.

The batch runner, the run counts, the cost limit and the estimate before the
first run are in `docs/specs/2026-11-16-cost-control.md` (R1-15). One exception to oldest first: mail with the label `SPAM` or from
a `hostile` sender needs no thread context, so it does not wait for the
older messages of its thread.

### 7.2 Clean text

- HTML becomes Markdown. Scripts, styles, tracking images and hidden elements
  go out.
- Quoted history goes out. The engine finds it from `In-Reply-To`, from the
  quote markers (`>`) and from the "On ... wrote:" lines of common clients.
- Signatures go out (the `-- ` line, and repeated end blocks in a thread).
- The engine extracts the text of PDF and plain-text attachments up to 1 MB
  of text for each message. Other attachments stay as a name, a type and a
  size.
- The clean text has a limit of 32,000 characters for each message. The engine
  cuts longer text and marks the cut.
- The engine reads at most 8 MiB of the raw message, so that one message
  stays inside the memory budget (M3). A longer message counts as cut text
  (section 7.3, rule 2), and parts after the limit are lost.

### 7.3 Fixed rules

Fixed rules run before the model. They are fast, free and the same each time.
The engine tries the rules in the order of the table. The first rule that
matches gives the result, and the next rules do not run.

| Order | Rule | Result |
|---|---|---|
| 1 | Hostile mail (section 6.7) | No model call and no item. The brief counts it (section 8). |
| 2 | Any sign that the message can hold other content: the clean text is cut at its limit (section 7.2); the message has an attachment; it has a calendar request (`text/calendar` with `METHOD:REQUEST`); it has `List-Id` or `List-Unsubscribe`; or the subject or the body matches a receipt or a statement pattern, or has a word of a request to pay or to act, of a contract, of a booking, of a ticket or of an event (for example "pay", "due", "contract", "reservation", "ticket", "event") | No fixed rule. The model reads the message, so that no record, request, invitation or feed item is lost. |
| 3 | A message from a `service` with a sign-in word (for example "sign in", "login", "verify", "passcode") | If the message has no promotion word (for example "discount", "coupon", "promo", "sale") and one clear code: signal of kind `code`. Else no fixed rule, and the model reads the message. |

Release 1 has no fixed list rule (issue #96) and no fixed invitation rule
(issue #97): list mail and invitations go to the model.

A code is a run of 4 to 8 digits bound to a code word ("code",
"verification", "one-time", "passcode"):

- After the code word, with only `:`, `-`, `=`, `#` or "is" between them:
  "Your code: 482913", "Your code is 482913".
- Before the code word, with only "is", "your", "the" or "a" between them:
  "482913 is your code".
- On the next line, if the line with the code word has no number and the
  next line has no code word: "Your code:" then "482913". If the line after
  that is also only a number of any length, the code is not clear:
  "Your code:", "1234", "5678" gives no code.

The message has one clear code when all bound numbers have one value, and each code-shaped number on a line with a code word, or on a line with a bound number, has that value ("Your old code was 5678" makes the code not clear).
Two values, or no bound number, give no clear code. A number with other
words between it and the code word is not bound.

#### Failure list of the fixed rules

The fixed rules read mail that Iris does not control. The safe result for
each case that a rule cannot settle is: no fixed item, and the model reads
the message. A fixed rule never guesses.

| Input | Failure mode | Result |
|---|---|---|
| A message with a hostile mark | A rule acts on text from an attacker | No item and no model call (rule 1). |
| A receipt, a statement, a payment request, a contract, a booking, a ticket or an event, also with a code | A code signal hides the record or the request | The model reads the message (rule 2). |
| A message with an attachment | A code signal hides a record in the file | The model reads the message (rule 2). |
| A message with a clean text cut at its limit | A code signal hides a record after the cut | The model reads the message (rule 2). |
| A calendar request | An invitation is lost | The model reads the message (rule 2, issue #97). |
| A list message (`List-Id` or `List-Unsubscribe`) | A code signal or a feed item hides other content | The model reads the message (rule 2, issue #96). |
| Sign-in mail with two code values (an account number, an old code, "1234 5678" on one line) | The wrong number becomes the code | No clear code: the model reads the message (rule 3). |
| Sign-in mail with an account number, a duration, a year or another number on a line with a code word ("Account: 123456 - code expires", "Your old code was 5678") | Another number becomes the code, or the wrong code is kept | No clear code: the model reads the message (rule 3). |
| Sign-in mail with the code in a form that the binding does not know ("Use 482913 to sign in") | The code is missed | No clear code: the model reads the message (rule 3). |
| A promotion code, or sign-in mail with a promotion word | A promotion becomes a sign-in code signal | The model reads the message (rule 3). |
| A request in words that the built-in word lists do not have, or in another language | A code signal hides the request | Known limit: the lists are short and in English (issue #95). |
| A very long line with many code words and numbers | The search takes too long | Only code words within 100 bytes of a number count, so the search is linear in the length of the line. |
| A rule item that fails the schema | An invalid item goes to the store | The rule gives no item, and the model reads the message. |
| All other input | | No fixed rule matches, and the model reads the message. |

The conversation rule runs for each message that is not hostile mail:

- The engine computes the conversation from the messages of the thread that
  are understood, or are in step 4 now, and that are not hostile mail and not
  gone, in the order of `internalDate`. It computes it again each time a
  message of the thread is understood, gone, or gets a hostile mark.
- If one or more of these messages is from you, from a `person` or from a
  `stranger`, the thread has one conversation. The first such message makes
  it and is its origin. The origin does not change after that, also when the
  origin message is gone (section 4.5). Each other message of the thread is
  an `update` source.
- `latest`, `messages`, `new` (messages after `thread.viewed_at`) and
  `your_turn` come from these messages.
- When each message of the conversation is gone, the conversation gets the
  status `gone`. It is not deleted.
- A thread with only `service` messages has no conversation. If the
  messages from you, a person or a stranger become hostile mail, and only
  `service` messages stay, the conversation also gets the status `gone`.
  While the origin message is only gone, the conversation keeps its status
  (section 4.5), and the messages that stay give its fields. A gone update
  source that becomes hostile mail stops being a source. The origin source
  always stays (section 4.5); a gone origin that is hostile mail does not
  keep the conversation from `gone`.
- If the origin message becomes hostile mail while it is not gone, the
  first message that counts from you, a person or a stranger becomes the
  origin, and gives the title and the quote. If there is no such message,
  the conversation gets the status `gone`.

`derived.by` is `rule`, and `derived.rule` names the rule.

### 7.4 The model

- The person chooses a hosted model with an own API key, or a local model
  through an HTTP endpoint. A hosted model is the default (manifesto). Setup
  tells the person that a hosted model receives the mail text.
- The engine sends one message at a time with its thread context: the clean
  text, the headers that the model needs, the sender kind and the known
  parties. The text goes inside a data block with a fixed delimiter.
  The instruction says that the block is data and that it contains no
  instructions. The headers include `List-Id` and `List-Unsubscribe`.
- The data block, with its delimiter lines, has a budget of 96,000 bytes,
  so that a local model with a
  small context window can take it. The headers (each value cut to 1,000
  bytes) go in first, then the clean text of the message. The budget cuts
  the clean text only when the headers and the text together pass it
  (32,000 characters can be 128,000 bytes). Existing items, attachment text, and then the earlier
  messages of the thread, newest first, fill the rest. Text that the
  budget cuts ends with `[cut]`.
- The engine, not the model, sets the fields that the headers give: the
  `issuer` of a record or a signal, and the `publisher`, `published` and
  `list` of a feed. In sent mail, the party of the sender is the
  owner, also when `From` is an alias.
- The `direction` of a commitment gives the side of the owner (`owner` for
  `you_owe`, else `counterparty`). The model chooses the other side, also
  a person who is not in the headers. The engine fills the other side only
  when the model gives no party with an address: with the sender, or in
  sent mail with the first `To`, `Cc` or `Bcc` recipient that is not an
  owner address (sent mail keeps `Bcc`). With no such party, the
  commitment is not valid. The other side is never the owner.
- The owner addresses are the address of the mailbox and, in sent mail,
  the `From` address (also an alias). A party with an owner address is the
  owner.
- The answer must have a boolean `phishing` and a list of at most 20
  items. When `phishing` is true, the list must be empty. Each party in the answer must be
  the owner or have an address. A party with the kind `self` and an
  address that is not an owner address is not clear. Else the answer is
  not valid.
- The model has no tools. It returns one JSON document: a list of items
  without the fields `model`, `id`, `created`, `updated` and `derived`, and a
  boolean `phishing`. Parties in the answer have an address, not a `p-` ID.
  The data lists at most 50 known persons as `Name <address>`, in the
  budget, so that the model can give the address of a person that the mail
  names only by name. They are the `person` senders in the `From`, `To` and
  `Cc` headers of the messages of the thread.
  An item can have `same_as`: the ID of an existing item that the engine gave
  to the model (section 7.5).
- The engine adds the missing fields, and puts the `p-` ID of the sender row
  of each address. Then it validates each item against the schemas. A quote
  that does not occur in the subject, the clean text or the attachment text
  makes the answer not valid. If an
  item is not valid, the engine tries one time again. If the second answer is
  not valid, the engine saves no item, adds 1 to `tries` of the message, and
  sets `retry_at` to 1 day later. After 3 tries, the engine stops, and the
  message stays not understood. The brief shows the count of such messages.
- If `phishing` is true, the engine sets the hostile mark `phishing` on the
  message, saves no item from it, and gives it the treatment of hostile mail
  (section 7.1, step 7).
- The budget counts each text from the mail (also an attachment name)
  after the escape of the data delimiter, so that the block that the
  client sends stays in the budget.
- An item with `same_as` adds nothing only when it is valid and has the
  form of the existing item. A `same_as` with another form makes the answer
  not valid. A `same_as` with an unknown ID is ignored, and the item is
  saved.

#### Failure list of the model step

The model answer and the model server are input that Iris does not
control. The safe result for each case that the engine cannot settle is:
no item, and the message stays not understood.

| Input | Failure mode | Result |
|---|---|---|
| One JSON document with a boolean `phishing` and valid items with quotes from the mail | | The engine saves the items. |
| `phishing` is true | The mail tries a trick | The hostile mark `phishing`, no item. |
| No JSON, a cut answer, no `phishing`, more than 20 items, a quote not in the mail, a party with no address, an item that fails the schemas | An answer that is not valid | One more try. A second answer that is not valid: no item, 1 more try in `tries`, `retry_at` 1 day later, a stop after 3 tries. |
| The model refuses to answer | A new try gets the same refusal | No item and no second call. The try counts as above. |
| The server refuses the call for this data (HTTP 400, 413, 422 and other 4xx, but not 401, 403 or 429) | A new call fails the same way | No item and no second call. The try counts as above. |
| A time-out, a lost response, HTTP 429 or 5xx, a refused key (401, 403) | The result is not known, or no call can work now | No item. No try counts. The message stays not understood, and a later run reads it again. A refused key stops the batch. |
| Data larger than the budget | A local model refuses a long message | The budget cuts the data (above). |
| All other answers | | Not valid, as above. |

- The worst result of a prompt injection is a wrong item. No item can start an
  action in release 1. In release 2, an item from mail can only become a
  proposal in a changeset (specification 2).
- `derived.model` records the model name and the version of the prompt, so
  that `iris rebuild --since-prompt <version>` can run the understanding step
  again on old mail.

### 7.5 Rebuild and corrections

- `iris rebuild` deletes the items that `derived.by` `rule` or `model` made,
  except pinned items (below), and runs understanding again on the mirror.
  `iris rebuild --since-prompt <version>` does the same only for the items
  whose `derived.prompt` is older than that version, and for their origin
  messages.
- A rebuild skips each gone message, because its text can be deleted. An item
  whose origin message is gone is pinned.
- A correction refers to its item by the item ID (`item_id`). A change of
  the status of an item by a person, or by an applied action of
  specification 2, is also a correction.
- A correction of a field (the verb `correct` of specification 2, section
  4) cannot change a field that the engine makes from the store on each
  change: the common fields (section 6.1), the `sender` ID of a party, and the fields of a conversation
  that the conversation rule makes (`thread`, `participants`, `role`,
  `latest`, `messages`, `new` and `your_turn`). The engine makes these
  fields again at the next message, so a correction of one would disagree
  with the store. The `sender` ID of a party follows its `address`: a
  correction of the address sets the ID of the sender row of the new
  address.
- The state verbs of specification 2 (`close`, `defer`, `decline`) change
  `status` and record the change as a correction too. The rule above does
  not apply to them.
- An item is pinned when it has one or more corrections, or when an open
  changeset or a ledger entry of specification 2 refers to it. A rebuild
  never deletes a pinned item and never changes its ID, so each correction
  and each reference stays on the same item.
- A rebuild runs understanding again on the source messages of a pinned item
  that are not gone. The engine gives the model the pinned items of the
  message as existing items. For each item that it finds, the model returns a
  new item, or the existing item with `same_as`. An item with `same_as` adds
  nothing. The engine removes `same_as` before it validates an item. A fixed
  rule matches an existing item by its form, its origin message and its
  rule.
- If a new item may be the same as a pinned item (same form and same origin
  message), the engine keeps both, and sets `duplicate_of` of the new item.
  The brief shows the pair once as a possible duplicate. From release 2, a
  person merges them, or keeps both with a correction of the new item, which
  pins it. The worst result is a duplicate in front of the person, never a
  lost or a moved correction.
- The schemas require `quote` on each origin source, so a person can see why
  an item exists. For an item from a fixed rule, the quote is the line that
  the rule matched (for example the subject or the code line). If that line
  is empty, the quote is `(no subject)`.

## 8. The first brief

The brief is a query on the items. It is not a separate store. Each query reads
the kind of a party from the sender row of its `p-` ID (the counterparty, the
issuer or the publisher), when the query runs. No query shows an item whose
status is `gone`.

Each query shows only an item whose `attention` state is `shown` or missing
(specification 2, section 6.4). The brief shows each `bundle` rule as one
line with the count of its items, and gives the count of the hidden items.
Each item of the brief is in one place only: its section, its bundle, or the
hidden count. A possible duplicate pair in one section goes where the
attention of the original puts it, and counts once there. The duplicate shows
with the original, whatever its own attention.

| Section | Query |
|---|---|
| You owe | Commitments with `direction` `you_owe` and `status` `open`, where the counterparty is a `person` or a `service`. Order by `due`, then by `created`. |
| Owed to you | Commitments with `direction` `owed_to_you` and `status` `open`, where the owner is a `person` or a `service`. Order by age. |
| Conversations | Conversations with `status` `active`, and `new` greater than 0 or `your_turn` true, with no `stranger` participant and no declined participant. |
| Records | Count for each record type. |
| Feed | Feed items with `status` `new`, by publisher. |
| Signals | Signals with `status` `active` and an `expires` time later than now. The engine sets `expired` at `expires`, so this also covers the time before the engine changes the status. |
| Strangers | Commitments with `status` `open` whose counterparty or owner is a `stranger`, and conversations with `status` `active` with a `stranger` participant. A stranger that the person declined at the gate (section 6.7) does not count. |
| Hostile | The count of hostile mail in the last 7 days, with no subject and no sender. |
| Not understood | The count of messages that the model failed 3 times (section 7.4). |

The sections do not overlap. A conversation with a `stranger` participant
shows only under Strangers, so each item shows one time and each section count
is exact. An item whose stranger the person declined, with no other stranger,
is in no section. The client uses the counts of the engine and does not remove
duplicates between sections.

The web client opens the brief with Due soon, after the live login codes
(card R3-08). Due soon holds each item of You owe that is due in 2 days or
less, or that is late, in the local day of the viewer. The engine does not
know that day (it has no time zone of the owner), so the client makes this
split from the You owe items, and the API keeps one You owe section. The
client shows You owe with the rest of its items, and with the engine count
less the Due soon items. The "All" link of You owe, and the link of Due soon,
give the engine count, because the full list holds both.

A watch whose mail came (card R5-07) leads the brief. The opening sentence
starts with one sentence that counts these mails ("The mail that you watched
for came." for one, "Two mails that you watched for came." for two), and one
card for each such
watch comes before the live login codes. The card opens the mail. Done
removes the watch. Near the end, "Iris watches for" lists the watches that
wait, and a quiet button starts a new watch. The client reads the watches
from `/api/watches`, not from the brief.

In release 1 the brief is read-only. Each line opens its source mail in one
step. The triage proposal ("Triage proposes 447 mailbox changes. Nothing is
applied.") is a list that the engine computes when the brief opens, in the
changeset format of specification 2, with each action in the state
`proposed`. The brief gives only the totals of the proposal: the count of
actions, and the count for each verb. The local API gives the full list
separately. Release 1 does not store it. From release 2, triage stores it as
a changeset, in the mode of the active policy (see below).

From release 2, the brief reads the stored triage changesets, and computes no
proposal. Its proposal totals count the messages of the triage actions that
wait for a person (`proposed` or `in_review`). The review lists these
changesets. The brief also gives the count of what Iris handled without the
person: the hidden items and the messages of the actions that a rule decided
without the person and that have one of the states `approved`, `held`,
`applied`, `failed`, `undone` or `denied`. A `failed` action stays, because it
can have changed some messages. An action in `in_review` waits for the person,
so the review lists it. An approved action that expires or whose changeset the
author withdraws gets a new decision (`expiry` or `author`) and changed
nothing, so it leaves the count and the list. One step shows the list of each, with the reason of each item and
action.

The triage proposal of release 1 (crate `iris-observe`) has these rules:

- Each rule gives one `archive` action on the messages with the label `INBOX`
  that are not gone. The rules are, in order: hostile mail, signals with the
  status `expired`, feed items, and records. A message goes only to the first
  rule that matches it.
- The engine reads the counts and the message IDs in one read transaction.
- Because release 1 does not store the proposal, its ID is an exception to
  the rule "unique in one install" of specification 2, section 5.1. The ID is
  `cs-` and the first 63 bits of the SHA-256 of the account and the message
  IDs of each rule. So the ID names the content: the same content gives the
  same ID, also at a later time, and a different content gives a different
  ID. A client must not use this ID as the ID of a stored changeset.

From release 2, the crate `iris-triage` stores the same proposal as a
changeset (specification 2, section 5.4):

- The author is `triage`, with the name `Iris` and the session
  `account-<row ID>`. Each account has its own changeset, because one action
  changes one account.
- Each rule gives one action for each 500 of its messages, with the reason of
  the rule. The changeset has its own reason. Nothing applies without a rule
  or an approval.
- A run uses the open triage changeset of the account. It submits a new
  revision only when the count, the verb, the reason or the targets of the
  actions that wait for a decision (`proposed` or `in_review`) changed. So an action in review does not change while the mailbox
  does not change.
- One run is one write transaction. It reads the IDs of one action at a time.
- A decision holds. A message that a triage action of the account named
  does not come back in a later proposal while that action is decided or
  approved (each state except `proposed`, `in_review` and `replaced`), also
  when its changeset is closed.
- In the policy mode `apply`, the active policy decides each `proposed`
  action of the new revision in the same transaction (specification 2,
  section 5.4, step 5), and the action keeps the reason of the decision. In
  the mode `observe`, each action stays `proposed`, except an action that a
  hard limit denies (specification 2, section 6.5): it gets `denied` by
  `limit`.

Failure list of a triage run. Supported input: the account row ID and the
forms, labels and senders in the store.

| Input or event | Failure mode | Safe result |
|---|---|---|
| A crash, an error or a refusal in a run | A draft revision that blocks each approval (spec 2, section 5.5), or half a proposal | The transaction rolls back. No changeset, revision or action of the run stays. |
| Two runs for one account at the same time, in one process or in two | Two open triage changesets for one account | Each run reads and writes in one transaction. In one process, the write lock of the store puts the runs in order. Across processes, SQLite refuses the write of the run whose snapshot is old, and that run gives an error. |
| A proposal with no action, while a triage changeset is open | A revision with no action, or open actions that triage no longer proposes | The run withdraws the changeset: its proposed and in_review actions become cancelled. If the changeset has an approved or held action, the run writes nothing, because a withdraw cancels that action too (issue #220). The open actions then wait for a person or the expiry. |
| A proposal with no action, and no open changeset | An empty changeset | The run writes nothing. |
| A large inbox | Memory that grows with the mailbox | The run keeps the IDs of one action (500 at most) in memory. |
| A message that leaves the inbox between the run and an apply | The apply changes a message that triage did not propose | Submit resolves the targets in the run. The apply (R2-02) checks each message again. |
| All other input | | The run gives an error and writes nothing. |

## 9. Memory

The design keeps memory flat when the mailbox grows:

- The sync streams each raw message to disk through a fixed buffer of 64 KiB
  for each fetch, at most 8 fetches at the same time. It decodes the base64url
  data of the answer while it reads, and never holds a full message.
- The understanding step parses at most one message at a time. It extracts
  the text of a PDF attachment only up to 10 MiB. A larger PDF stays as a
  name, a type and a size.
- The engine never keeps a list of all messages in memory. The verify step
  and the full resync compare IDs page by page against SQLite. They mark the
  IDs that a list names in the table `sync_seen` on disk.
- SQLite uses at most 2 MiB of page cache for each connection, and 3
  connections.

A change that adds a cache must say its size in the pull request.

## 10. Test plan

- The fake Gmail server (tracker item R0-08) and the synthetic mailbox
  (R0-07) test the first sync, the incremental sync, the 404 resync and
  verify, with no network.
- A test deletes a message at the fake server that is the only source of an
  item. After the 30 days, the item has the status `gone` and its source still
  points to the tombstone.
- A test drops the connection in the middle of the first sync and checks that
  the next start fetches only the rest.
- A test runs the content cleanup during a live backup, and checks that each
  message row with a `content_sha256` in the backup has its content file.
- A test changes one byte of a content file, and checks that verify reports 1
  difference and fetches the message again.
- A test adds and then deletes a message at the fake server before an
  incremental sync, and checks that the sync completes and saves the new
  history ID.
- A test deletes a message at the fake server between the list and the fetch
  of the first sync, and checks that the mirror becomes ready.
- A test restores a live backup to a new folder, and checks that the engine
  reads the secrets and the policy.
- A test deletes a message at the fake server during step 7 of a full resync,
  and checks that the resync completes and that the message is gone after
  step 8.
- A test stops the engine during step 5 of a full resync. After the start,
  the resync continues from `H1`, and no incremental sync runs before it ends.
- A test renames and deletes a label at the fake server, and checks that the
  next incremental sync corrects the label rows, and that verify reports 0
  differences.
- A test with a message that has a forged `Authentication-Results` header
  below the header of Gmail: the engine uses only the header of Gmail.
- A test with a message with the label `DRAFT`: understanding makes no item
  from it.
- A test runs understanding again on one message, and checks that the items
  of the message are not duplicated.
- A test changes labels at the fake server between two syncs and checks that
  verify reports 0 differences.
- A test changes a label at the fake server while verify lists the labels,
  and checks that verify reports 0 differences.
- A test makes an item with an origin message and an update message, and
  deletes the origin at the fake server. After the 30 days, the item keeps
  its status, its origin source cites the tombstone, and the item is valid
  against the schema.
- A test sends a `messageAdded` record for a message that the mirror has, and
  checks that the sync changes the label set and does not fetch the message.
- A test marks a message as gone, and then a full resync finds it in the
  list. The message comes back, and its items run understanding again.
- A test sends a message with the display name of a known person, another
  address and a DMARC fail. No text goes to the model, and the sender row of
  the known person does not change.
- A rebuild test with an item that an open changeset refers to. After the
  rebuild, the item has the same ID.
- These tests need features that the fake server (tracker item R0-08) does
  not have yet. Add them to the fake server before the sync code is written:
  - A delete of a message, a change of labels, and a rename and a delete of a
    label, with the history records `messageDeleted`, `labelAdded` and
    `labelRemoved`, and the filter `historyTypes`.
  - The `labelIds` filter and the `includeSpamTrash` parameter of
    `messages.list`, with messages in `SPAM`, `TRASH` and `DRAFT`, and user
    labels.
  - For each format of `messages.get`, only the fields that the Gmail
    documentation promises, so that a test cannot pass on a field that Gmail
    does not send.
- The schema test (`crates/iris-spec`) validates the sample documents in
  `schema/examples/` against the schemas, and checks that each sample in
  `schema/examples/invalid/` fails.
- A rebuild test of release 2 with one message that gives two commitments. A
  person corrects the due date of one. After the rebuild, that commitment has the
  same ID and the corrected date, and the other commitment has no correction.
- The rebuild tests use a fixed fake model, which gives the same answer for
  the same input.
- The understanding tests use the ground-truth file of the synthetic mailbox.
  They measure the share of messages with the correct form. Release 1 sets the
  target after the first measure.
- The hostile samples of the synthetic mailbox check that no hostile body goes
  to the model, and that no item comes from hostile mail.

## 11. Open questions

1. OAuth client for self-hosted users (tracker item R1-01). Decided on
   2026-11-15: each user makes an own OAuth client, published to "In
   production" without verification. See
   `docs/decisions/2026-11-google-sign-in.md`.
2. Contacts. The rule for `person` uses only sent mail. The Google People API
   can add contacts, but it needs one more scope. Version 0 does not use it.
3. One conversation for each thread. Gmail threads by subject, and a person
   can talk about two subjects in one thread. Version 0 keeps one conversation
   for each thread.

## Assumptions

- The Gmail API v1 keeps the behavior that this specification uses: `history.list`
  returns HTTP 404 for a history ID that is too old, and `messages.list`
  accepts `includeSpamTrash`. Verify against the Google documentation before
  the provider client is written.
- The Gmail quota page (checked on 2026-11-15) gives 6,000 quota units each
  minute for each user of a project. `messages.get` costs 20 units, or 60
  units with `format=raw`. `messages.list` costs 5 units, `history.list` 2
  units and `users.getProfile` 1 unit. A project that used the API before
  May 2026 can keep its earlier quota. The provider client reads the limit
  from a setting, so that a change needs no new release.
- With these numbers, the first sync of 100,000 messages costs 6,000,000
  units and takes about 17 hours at the quota limit. The newest-first order
  of section 4.2 lets understanding start on recent mail in the first
  minutes. The list calls of a full resync or a verify cost about 1,000
  units for each 100,000 label memberships.
- `messages.get` with `format=raw` returns `labelIds` and `internalDate` with
  the raw data. Verify this against the real API before the sync code is
  written. The Gmail documentation promises only the ID and the labels for
  `format=minimal`, so this specification uses `format=minimal` only for
  labels and for a 404.
- Gmail keeps the change history "typically" for at least one week, and in
  rare cases for only a few hours (Gmail sync guide). If it keeps it less
  than the 60-second sync interval in practice, each sync becomes a full
  resync, and the design must change.
- Gmail deletes a message in the trash for good after 30 days (Gmail Help
  Center, not the API documentation). The mirror follows each delete through
  the history, so the design does not depend on this number.
- The `bundled` SQLite of `rusqlite` has FTS5. The engine checks this at start
  and stops with a clear error if FTS5 is not present.
- A mailbox of 100,000 messages has a content folder of a few GB. The data
  folder is on a disk with enough free space. The engine shows the free space
  at setup.
- The owner of the mailbox is one person. Shared mailboxes and delegated
  access are not in scope.
- The engine runs on one machine. Two engines on the same data folder are an
  error, and a lock file prevents them.
- The understanding model can return valid JSON for the schemas in most
  cases. If the rate of invalid answers is high, the prompt or the schema
  changes, not this format.
