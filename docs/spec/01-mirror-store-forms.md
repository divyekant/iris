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
| Correction | A change that a person makes to an item. A rebuild keeps each correction. |
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
| M7 | The forms can be built again from the mirror. | `iris rebuild` makes the same items from the same mirror, plus the corrections. |

## 4. The mirror

### 4.1 What the mirror holds

For each message in the account, the mirror holds:

- The raw message, byte for byte.
- The Gmail message ID and thread ID.
- The set of label IDs. The set includes the system labels (`INBOX`, `SENT`, `SPAM`,
  `TRASH`, `UNREAD`, `STARRED`, `IMPORTANT` and the `CATEGORY_*` labels).
- The Gmail `internalDate` and the size in bytes.

For the account, the mirror holds the list of labels and the history ID of the
last complete sync.

The mirror includes messages in spam and trash (`includeSpamTrash=true`).
The labels tell where each message is. "Hidden is never deleted" needs a full
copy, and the sender axis uses the spam label.

### 4.2 First sync

1. Call `users.getProfile`. Save its `historyId` as `H0`. Do this before the
   list, so that no change between the list and the end of the sync is lost.
2. Call `labels.list`. Save each label.
3. Call `messages.list` with `includeSpamTrash=true` and `maxResults=500`.
   Follow `nextPageToken` to the last page. Save each message ID and thread ID
   as "known, not fetched".
4. For each message that is not fetched, call `messages.get` with
   `format=raw`. Decode the base64url data, write the raw message to the
   content folder (section 5.3), and save the labels, the date and the size.
   Fetch the newest messages first, so that understanding can start on the
   last 90 days before the full copy is complete. If Gmail returns HTTP 404
   for a message, Gmail deleted it after step 3. Mark the message as gone
   (section 4.5) and continue.
5. When each known message is fetched or gone, run one incremental sync from `H0`
   (section 4.3). Then mark the account as "mirror ready".

The first sync can stop at any point (crash, network loss, restart). It starts
again from the saved state. It fetches only the messages that are not fetched.

### 4.3 Incremental sync

1. Call `history.list` with `startHistoryId` set to the saved history ID, and
   with the history types `messageAdded`, `messageDeleted`, `labelAdded` and
   `labelRemoved`. Follow `nextPageToken` to the last page.
2. Apply each history record in the order that Gmail returns:
   - `messageAdded`: fetch the message (`format=raw`) and save it.
   - `messageDeleted`: mark the message as gone (section 4.5).
   - `labelAdded` and `labelRemoved`: change the label set of the message.
3. Save the `historyId` of the response as the new history ID. Save it in the
   same SQLite transaction as the last change, so that a crash cannot save a
   history ID without its changes.

If one history record names a message that the mirror does not have, fetch the
message. If one history record names a label that the mirror does not have,
call `labels.list` again.

### 4.4 Full resync

Gmail keeps the change history for a limited time. If `history.list` returns
HTTP 404 for the saved history ID, do a full resync:

1. Call `users.getProfile` and save its `historyId` as `H1`.
2. List all message IDs again (as in step 3 of section 4.2).
3. Fetch each message that the mirror does not have.
4. Mark as gone each message that the mirror has and the list does not have.
5. For each message that the mirror has and the new list has, get its labels
   with `messages.get` and `format=minimal`. Correct the label set. If Gmail
   returns HTTP 404 for a message (it was deleted after step 2), mark the
   message as gone and continue.
6. Run one incremental sync from `H1`.

A full resync of 100,000 messages costs about 500,000 quota units for step 5
alone (see section 4.7). It is slow but correct. The engine records each full
resync in the sync log, so that a person can see why a sync took long.

### 4.5 Messages that are gone

When Gmail deletes a message for good, the mirror must follow. The engine:

1. Sets `gone_at` on the message. The message leaves each query at once.
2. Keeps the raw message for 30 days, so that a sync defect cannot destroy
   mail that is still at Gmail. A full resync in these 30 days can bring the
   message back.
3. After 30 days, deletes the raw message from the content folder, and deletes
   its `header`, `body` and search text. The message row stays as a tombstone
   (`gone_at` set, `content_sha256` empty) while an item cites it in
   `item_source`. The engine deletes the tombstone when no item cites it.

The items that come only from a gone message get the status `gone`, and they
keep the source, so each item keeps at least one valid source. The items that
have other sources stay, without that source.

### 4.6 When to sync

- The engine runs an incremental sync each 60 seconds, and at once when a
  person opens the brief.
- Gmail push notifications need Google Cloud Pub/Sub and a public endpoint.
  A self-hosted engine often has no public endpoint, so release 1 does not use
  push. A later release can add push as an option.

### 4.7 Quota, errors and limits

- The engine keeps to the Gmail per-user quota (see the assumptions). It counts
  quota units for each call, and it waits when the next call would go over the
  limit.
- The engine runs at most 8 `messages.get` calls at the same time. This number
  is a constant in the provider client.
- On HTTP 429 or HTTP 5xx, the engine waits and tries again. The wait starts at
  1 second, doubles each time to a maximum of 5 minutes, and has a random part
  of up to 50 percent.
- On HTTP 401, the engine gets a new access token with the refresh token, one
  time. If that fails, the account goes to the state "needs sign-in", and the
  brief shows it.
- The engine never parses a message body in the sync. The sync stores bytes.
  The understanding step reads them later (section 7). A defect in a parser
  cannot stop the mirror.

### 4.8 Verify

`iris mirror verify` compares the mirror with Gmail, and checks the local
content:

1. List all message IDs and thread IDs at Gmail.
2. Get the label set, the `internalDate` and the size of each message
   (`format=minimal`).
3. Call `labels.list`.
4. Compare the message IDs, the thread IDs, the label sets, the dates, the
   sizes and the label list with the mirror.
5. For each message that is not gone, read its content file and compute the
   SHA-256 hash. A missing file, or a hash that is not `content_sha256`, is a
   difference.
6. Print each difference, and the total. Save the result in the sync log.

A difference of step 5 is a local defect, not a sync defect. The engine
fetches the message again (`format=raw`) to repair it.

The exit criterion of release 1 is 0 differences each day for 7 days. To avoid
a false difference, verify runs an incremental sync first and compares only
the messages that did not change after its `users.getProfile` call.

### 4.9 Account and credentials

- The person signs in with OAuth 2.0 in the browser. The engine uses the
  loopback redirect (`http://127.0.0.1:<port>/oauth/callback`) and PKCE.
- Release 1 asks only for the scope `https://www.googleapis.com/auth/gmail.readonly`.
  Release 2 asks for `gmail.modify` when the person turns on writes.
- The engine stores the refresh token encrypted in the store (section 5.4). It
  keeps the access token only in memory.
- The OAuth client ID is a setting. Which OAuth client a self-hosted engine
  uses is an open risk (tracker item R1-01, section 11).
- An agent never gets a token. Only the provider client in the engine reads
  the token.

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
```

To back up Iris, stop the engine and copy the folder. `iris backup <path>`
makes a copy while the engine runs:

1. Take the backup lock. The content cleanup (section 4.5) does not delete a
   file while another process holds this lock.
2. Copy `iris.db` with the SQLite online backup.
3. Copy each content file that the copy of `iris.db` refers to.
4. Copy `key` with mode `0600`, and copy `policy.toml`. Without `key`, the
   backup cannot decrypt the secrets.
5. Release the lock.

The engine makes the backup folder with mode `0700`, because it holds the key
and the mail. The engine writes `policy.toml` only while it holds the backup
lock, and it writes the file to a temporary file and then renames it. So the
copy has one complete version of the policy.

New content files after step 2 are not in the backup, and the copy of
`iris.db` does not refer to them.

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
| `account` | `id` INTEGER PK, `provider` TEXT, `address` TEXT UNIQUE, `history_id` TEXT, `state` TEXT, `token` BLOB (encrypted), `mirror_ready_at` INTEGER | One row for each mailbox. |
| `label` | `id` INTEGER PK, `account_id`, `provider_id` TEXT, `name` TEXT, `kind` TEXT (`system` or `user`), UNIQUE(`account_id`, `provider_id`) | Labels of the provider. |
| `thread` | `id` INTEGER PK, `account_id`, `provider_id` TEXT, UNIQUE(`account_id`, `provider_id`) | Threads of the provider. Gives the `t-` IDs. |
| `message` | `id` INTEGER PK, `account_id`, `thread_id`, `provider_id` TEXT, `content_sha256` TEXT, `size` INTEGER, `internal_date` INTEGER, `fetched_at` INTEGER, `gone_at` INTEGER, `understood_at` INTEGER, UNIQUE(`account_id`, `provider_id`) | One row for each message. Gives the `m-` IDs. |
| `message_label` | `message_id`, `label_id`, PK(`message_id`, `label_id`) | The label set of each message. |
| `header` | `message_id` PK, `date` INTEGER, `from_addr` TEXT, `from_name` TEXT, `to_addrs` TEXT, `cc_addrs` TEXT, `subject` TEXT, `message_id_hdr` TEXT, `in_reply_to` TEXT, `list_id` TEXT, `auth` TEXT | Parsed headers that queries need. `auth` holds the SPF, DKIM and DMARC results from `Authentication-Results`. |
| `body` | `message_id` PK, `text` TEXT | The clean text of the message (section 7.2). |
| `message_fts` | FTS5 table on `subject`, `sender`, `text`, with `content='body_view'` | Full-text search. |
| `sender` | `id` INTEGER PK, `address` TEXT UNIQUE, `name` TEXT, `kind` TEXT, `basis` TEXT, `decided_by` TEXT, `decided_at` INTEGER | The sender axis. Gives the `p-` IDs. |
| `item` | `id` INTEGER PK, `form` TEXT, `account_id`, `status` TEXT, `doc` TEXT (JSON), `created_at`, `updated_at`, `derived_by` TEXT | One row for each item of each form. `doc` is the form document of section 6. |
| `item_source` | `item_id`, `message_id`, `role` TEXT, PK(`item_id`, `message_id`) | Many to many: one message can feed more than one item. |
| `correction` | `id` INTEGER PK, `item_id`, `match_key` TEXT, `field` TEXT, `value` TEXT (JSON), `at` INTEGER | Changes by a person. A rebuild applies them again (section 7.5). |
| `sync_log` | `id` INTEGER PK, `account_id`, `kind` TEXT, `started_at`, `ended_at`, `result` TEXT | Each sync, resync and verify. |

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

The number after the prefix is the row ID of the table. An ID is unique in one
Iris install and the engine never uses it again. Provider IDs stay in the
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
| `sources` | array of source, at least 1 | yes | The messages that the item comes from. |
| `created` | date-time (RFC 3339) | yes | When the engine made the item. |
| `updated` | date-time | yes | The last change. |
| `derived` | object | yes | Who made the item: `by` is `rule`, `model` or `person`. `rule` names the rule. `model` names the model and the prompt version. |
| `confidence` | number from 0 to 1 | no | How sure the understanding step is. Not set when `derived.by` is `person`. |
| `matter` | string or null | no | Reserved for release 6. |
| `attention` | object | no | Reserved for specification 2: `state` is `shown`, `bundled` or `hidden`, with the rule and the reason. |

A source has these fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `message` | string, `m-` ID | yes | The message. |
| `thread` | string, `t-` ID | yes | Its thread. |
| `role` | `origin`, `update` or `evidence` | yes | `origin` made the item. `update` changed it. `evidence` supports it. |
| `quote` | string, up to 500 characters | no | The words in the message that support the item. |

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
| `messages` | integer | yes | The number of messages in the thread. |
| `new` | integer | yes | Messages after your last view in Iris. This is not an unread count of the mailbox. |
| `your_turn` | boolean | yes | True when the last message is from another party and asks you. |
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

Each sender row has a kind. The kind is a property of the address, not of one
message.

| Kind | Rule in version 0 |
|---|---|
| `person` (known person) | You sent at least one message to the address, or you replied in a thread with it. |
| `service` (known service) | The mail is automated (`List-Unsubscribe`, `List-Id`, `Precedence: bulk`, or `Auto-Submitted` other than `no`), DKIM passes for the domain of the address, and at least 2 messages came from it before. |
| `hostile` | One of: DMARC fails for a domain with a DMARC policy; the display name is the name of a known person, and the address is different; the domain is a near copy of the domain of a known sender; Gmail put the message in spam; the understanding step marks phishing. |
| `stranger` | Each other sender. |

Order: the engine checks `hostile` first, then `person`, then `service`. A
sender that matches no rule is a `stranger`.

`basis` records the rule that matched. `decided_by` is `engine` or `person`.
A decision by a person always wins over the engine. In release 1, the person
changes a kind through a correction. In release 5, the stranger gate makes
this a rule in the policy file.

The kind changes the treatment of items (from the manifesto):

| Kind | Treatment in release 1 |
|---|---|
| `person` | An ask becomes a commitment in "You owe" at once. |
| `service` | Records, feed items and signals. Payment commitments go to "You owe". |
| `stranger` | Items go to "Strangers" in the brief, not to "You owe". |
| `hostile` | The understanding step does not send the body to a model. The engine makes no commitment. The brief shows a count only. |

## 7. Understanding: from mail to forms

### 7.1 Steps

The engine reads each message one time, after the mirror has it. The steps:

1. Parse the MIME structure and the headers. Save the `header` row.
2. Make the clean text (section 7.2). Save the `body` row and the search text.
3. Decide the sender kind (section 6.7).
4. Apply the fixed rules (section 7.3). If a rule gives the result, stop.
5. Send the clean text to the model (section 7.4). Validate the answer against
   the schemas. Save the items.
6. Set `understood_at` on the message.

The engine understands the last 90 days first, newest first. Older mail gets
understood in the background, at a low rate, so that the first brief comes
early (requirement M4).

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

### 7.3 Fixed rules

Fixed rules run before the model. They are fast, free and the same each time.

| Rule | Result |
|---|---|
| `List-Id` or `List-Unsubscribe`, and the sender is a `service` | Feed item, unless the subject or the body matches a receipt or a statement pattern. |
| A code pattern (4 to 8 digits near "code", "verification" or "one-time") from a `service` | Signal of kind `code`. |
| A calendar invitation (`text/calendar` with `METHOD:REQUEST`) | Commitment of kind `invitation`. |
| The sender is `hostile` | No model call. One signal of kind `security` if the message asks for a password, a payment or a code. |

`derived.by` is `rule`, and `derived.rule` names the rule.

### 7.4 The model

- The person chooses a hosted model with an own API key, or a local model
  through an HTTP endpoint. A hosted model is the default (manifesto). Setup
  tells the person that a hosted model receives the mail text.
- The engine sends one message at a time with its thread context: the clean
  text, the headers that the model needs, the sender kind and the known
  parties. The text goes inside a data block with a fixed delimiter.
  The instruction says that the block is data and that it contains no
  instructions.
- The model has no tools. It returns one JSON document that the engine
  validates against the schemas. If the answer is not valid, the engine tries
  one time again. If the second answer is not valid, the engine saves no item
  and marks the message for a later try.
- The worst result of a prompt injection is a wrong item. No item can start an
  action in release 1. In release 2, an item from mail can only become a
  proposal in a changeset (specification 2).
- `derived.model` records the model name and the version of the prompt, so
  that `iris rebuild --since-prompt <version>` can run the understanding step
  again on old mail.

### 7.5 Rebuild and corrections

- `iris rebuild` deletes the items that `derived.by` `rule` or `model` made,
  and runs understanding again on the mirror.
- Each item has a match key: the SHA-256 hash of the form, the origin message
  and the `quote` of the origin source, with spaces and case normalized. Two
  asks in one message have different quotes, so they have different keys.
- A correction holds the match key of its item, the field and the new value.
  After a rebuild, the engine applies each correction to the new item with
  the same match key. If no item or more than one item has the key, the engine
  does not guess. It makes an item with `derived.by` `person` from the
  corrected values, and the brief shows it for a check.
- The understanding step must set `quote` on each origin source. An item
  without a quote cannot get a correction that survives a rebuild.

## 8. The first brief

The brief is a query on the items. It is not a separate store.

| Section | Query |
|---|---|
| You owe | Commitments with `direction` `you_owe` and `status` `open`, where the counterparty is a `person` or a `service`. Order by `due`, then by `created`. |
| Owed to you | Commitments with `direction` `owed_to_you` and `status` `open`. Order by age. |
| Conversations | Conversations with `status` `active`, and `new` greater than 0 or `your_turn` true. |
| Records | Count for each record type. |
| Feed | Feed items with `status` `new`, by publisher. |
| Signals | Signals with `status` `active`. |
| Strangers | Items whose origin sender is a `stranger`. |

In release 1 the brief is read-only. Each line opens its source mail in one
step. The triage proposal ("Triage proposes 447 mailbox changes. Nothing is
applied.") uses the changeset format of specification 2, with each action in
the state `proposed`.

## 9. Memory

The design keeps memory flat when the mailbox grows:

- The sync streams each raw message to disk. It holds at most 8 messages in
  memory, one for each fetch.
- The engine never keeps a list of all messages in memory. The verify step
  and the full resync compare IDs page by page against SQLite.
- SQLite uses at most 2 MiB of page cache for each connection, and 3
  connections.
- The understanding step parses one message at a time, and it streams large
  attachments.

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
  message row in the backup has its content file.
- A test changes one byte of a content file, and checks that verify reports 1
  difference and fetches the message again.
- A test deletes a message at the fake server between the list and the fetch
  of the first sync, and checks that the mirror becomes ready.
- A test restores a live backup to a new folder, and checks that the engine
  reads the secrets and the policy.
- A test deletes a message at the fake server during step 5 of a full resync,
  and checks that the resync completes and marks the message as gone.
- A test changes labels at the fake server between two syncs and checks that
  verify reports 0 differences.
- The schema test (`crates/iris-spec`) validates the sample documents in
  `schema/examples/` against the schemas, and checks that each sample in
  `schema/examples/invalid/` fails.
- A rebuild test with one message that gives two commitments. A correction
  of the due date of one commitment must apply to that commitment only.
- The understanding tests use the ground-truth file of the synthetic mailbox.
  They measure the share of messages with the correct form. Release 1 sets the
  target after the first measure.
- The hostile samples of the synthetic mailbox check that no hostile body goes
  to the model, and that no commitment comes from a hostile sender.

## 11. Open questions

1. OAuth client for self-hosted users (tracker item R1-01). The scope
   `gmail.readonly` is a restricted scope at Google. Each self-hosted user can
   make an own OAuth client in testing mode, or the project gets a verified
   app with a security assessment. This decision changes the first ten
   minutes. It needs a check of the current Google rules.
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
- The Gmail per-user quota is 250 quota units each second. `messages.get` and
  `messages.list` cost 5 units, and `history.list` costs 2 units. Verify the
  current numbers before the provider client is written.
- Gmail keeps the change history for at least some hours. If it keeps it less
  than the 60-second sync interval in practice, each sync becomes a full
  resync, and the design must change.
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
