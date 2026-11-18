# Spec: the Outlook provider (Microsoft Graph)

- Date: 2026-11-17
- Card: R6-04
- Status: Draft

## Problem

Iris mirrors only Gmail. Many people keep their mail in Outlook.com or in
Microsoft 365. These mailboxes have no Gmail API. Microsoft gives one API
for them: Microsoft Graph (Graph). Graph has another model of mail than
Gmail: each message is in one folder, a message has categories in place of
labels, and the change feed is a delta query for each folder, not one
history ID for the mailbox.

## Goal

1. A crate `crates/iris-graph` holds a thin Graph mail client for the
   mirror: folders, delta queries, the raw MIME of a message, and the write
   calls (read state, flag, categories, move, draft and send).
2. A crate `crates/iris-fakegraph` holds a fake Graph server on top of the
   synthetic mailbox. CI tests the client against the fake. No test calls
   Microsoft.
3. `crates/iris-auth` signs in to a Microsoft account with the
   authorization code flow and PKCE, as a public client (no secret).
4. After the provider interface (card R6-10) merges, the Graph client
   implements that interface, and the engine syncs an Outlook account.

Card R6-04 is done when this spec merges. The build cards of "Plan" end
when an Outlook mailbox syncs with zero differences against the fake
server (R6-14). The release gate is 7 days with zero differences
on a real Outlook mailbox on dk's Mac, after R1-07 ends.

## Not in scope

- Exchange Web Services, IMAP to Outlook and on-premises Exchange.
- Calendar, contacts and tasks.
- The Exchange archive mailbox. A move to the archive mailbox changes the
  immutable ID (assumption A3), so the mirror sees a delete.
- Category colors. R6-14 reads the names of the category list
  (`outlook/masterCategories`) for the label list, with the scope
  `MailboxSettings.Read` (section 7).
- One shared Iris app registration. See "Options not chosen".

## Design

### 1. The client

`iris_graph::Graph` follows `iris_gmail::Gmail`: it is synchronous, it uses
`ureq` (the one HTTP stack of the program), and each call takes the access
token as a parameter. The client stores no token.

Each request:

- goes to `https://graph.microsoft.com/v1.0/me/...`, or to a test base URL;
- sends `Prefer: IdType="ImmutableId"`, so that a message keeps its ID when
  it moves between folders (A3);
- waits for a free place in a gate of 4 requests at the same time for each
  client (A5), because Graph counts each app and mailbox pair.

A `@odata.nextLink` or a `@odata.deltaLink` is a full URL. The client
parses the link and calls it only if its scheme and its authority (host
and port, with no user part) are equal to those of the base URL, and its
path starts with the path of the base URL. A text prefix is not enough:
`https://graph.microsoft.com@attacker.example/` starts with the base text.
Else the client gives `Error::Decode` and sends no request. The client
also turns off the redirects of `ureq`: an HTTP 3xx answer is an
`Error::Api` with that status, and the client follows no `Location`.
Thus a changed answer cannot send the token to another host.

Waits after a refusal:

- HTTP 429 and HTTP 5xx of a read: wait for `Retry-After` when the answer
  has it, else 1 second, then double up to 5 minutes, plus up to 50 percent
  at random (A5), and try again.
- A write makes one request only. The client never tries a write again,
  also not after 429 or 503. A time-out, a lost answer, 429, 5xx and an
  unfollowed 3xx of a write give an unknown result
  (`Error::write_result_unknown`). An accepted write whose answer is not
  in the Graph shape, or does not show the new state, also gives an
  unknown result. The caller reconciles it with a read (see "External
  writes").
- HTTP 401: `Error::Unauthorized`. Graph refused the token before it
  ran the call, so a 401 is a definite rejection, not an unknown result,
  also for a write. The caller gets a new access token one time and makes
  the call one more time with it (spec 2 section 7.2). This is the only
  second request of a write. A second 401 stops the call: the account
  needs a new sign-in.
- HTTP 404: `Error::NotFound`.
- Other status: `Error::Api { status, code, message }`, with `code` from the
  Graph error body `{"error":{"code","message"}}` (A8).

### 2. Folders

`folders(token)` reads `/me/mailFolders?includeHiddenFolders=true` and, for
each folder with `childFolderCount > 0`, its `childFolders` with
`includeHiddenFolders=true` too (Graph leaves hidden child folders out by
default), page by page. A search folder (`@odata.type` is
`#microsoft.graph.mailSearchFolder`) is a view: its messages are in
other folders. The client leaves each search folder and its children out
of the list, so that each message has one folder.
It gives each folder with its ID, parent ID, display name, the counts, and
the well-known name when the folder is one of `inbox`, `sentitems`,
`drafts`, `deleteditems`, `junkemail`, `archive` or `outbox` (A7). It finds
the well-known names with one call `GET /me/mailFolders/{name}` each.

The client does not use `mailFolders/delta`. Microsoft does not say if it
gives child folders at all depths (A2). A mailbox has tens of folders, so a
full read at each sync costs a few calls.

A folder can go away (the person deletes it, or moves it out of the
mailbox tree). If a folder of the mirror is not in the new list, the sync
drops its delta link, and each mirror message of that folder gets the
read by ID of section 3: 404 means gone, a message means a move to its
new folder. A renamed or moved folder keeps its ID, so only its name or
parent changes in the mirror.

### 3. Delta of the messages of one folder

`delta(token, folder, link)`:

- With no link: `GET /me/mailFolders/{id}/messages/delta` with
  `$select=id,conversationId,parentFolderId,categories,isRead,flag,isDraft,receivedDateTime,internetMessageId`
  and `Prefer: odata.maxpagesize=200`.
- With a link: the stored `nextLink` or `deltaLink`, as it is.

A page gives a list of changes and one of two links:

- `More(nextLink)`: call it for the next page.
- `Done(deltaLink)`: the end of this round. Keep it for the next sync
  (see "Commit" below).

A change is one of:

- `Message(m)`: the message is in the folder now, with the fields above.
  Graph can give the same message more than one time (A1). The last one
  in the page order is the current state.
- `Removed(id)`: an item with `@removed`. The message left the folder: a
  delete or a move (A1). The caller does not delete the message at once:
  a move gives `Removed` in one folder and `Message` in another, with the
  same immutable ID. The rounds of two folders are not one snapshot: if
  the round of the new folder ran before the move, only the `Removed` of
  the old folder shows the move, and a message can move again after a
  round gave it as `Message`. Thus, after the round of each folder, the
  caller reads each removed message by ID with `message(id)`
  (`GET /me/messages/{id}?$select=...`), also when a round gave it as
  `Message`: HTTP 404 means gone, and a message gives its folder now. The
  message stays in the mirror until this read gives 404.

Commit: the new delta links of a sync become durable only in the same
transaction as all the changes of the sync and the results of the reads
by ID. Until that commit, the store keeps the old links. A crash before
the commit loses nothing: the next sync starts from the old links, and
Graph gives the changes again (A1). The changes and the reads of one
sync are a set of IDs with their state; the sync keeps them on disk in
the store, page by page, as `iris-sync` does for Gmail, so memory stays
flat.

A token that Graph does not accept any more gives HTTP 410, or another 4xx
with a code such as `syncStateNotFound` (A1). The client gives
`Error::DeltaReset` for 410 and for each 4xx of a call with a stored link,
except 401, 403 and 429. The provider gives `HistoryExpired` for it, and
also when a folder of the cursor is not in the folder list any more. The
sync then does the full resync of spec 1 section 4.4 for the whole
account: the list, the read by ID of each mirror message that the list
does not have (404 means gone, a message gives its folder now), and the
label lists. A round with no link lists only the messages that are in the
folder now, so a full resync is the only path that finds a message that
went away while a link was expired. Known limit with its own issue: one
expired link costs a resync of all folders.

### 3.1 The provider calls (R6-14)

`iris_graph::Outlook` implements `iris_provider::Provider` on these
rules:

- The cursor is JSON: `{"v":1,"links":{"<folder id>":"<deltaLink>"}}`.
  `profile` gives the cursor with no links, because Graph has no cheap
  cursor of now. The next `changes` then runs a new round of each
  folder: each message gives `Change::Added` with its labels now. Known
  limit with its own issue: a message that is deleted after the list of
  a first sync or a full resync, and before its first `changes`, stays in
  the mirror until the next verify finds it (an extra message starts a
  full resync).
- `changes` gives one delta page of one folder for each call. The page
  token holds the folder IDs of the call, the folder of the next page,
  its `nextLink` and the delta links of the folders whose round ended.
  Each `Removed` gets the read by ID in the same call: a message gives
  `Change::Added` with its labels now (a move), 404 gives
  `Change::Deleted`. The last page gives the new cursor, so the sync
  stores all links in one transaction with the last changes.
- `messages` with no label lists all mail of all folders, newest first:
  `GET /me/messages` with `$orderby=receivedDateTime desc`, so that the
  first sync fetches the newest threads first (spec 1 section 4.2). The
  page token is the `nextLink`. Assumption, evidence `pending` (release
  gate): this list has the messages of each folder. If it misses one,
  the first `changes` after the first sync lists it in the new round of
  its folder and the sync fetches it then.
- `messages` with a folder label lists that one folder with a delta
  round with no link. For another label (`UNREAD`, `STARRED`, a
  category) it reads all folders that way and keeps the messages that
  have the label. These lists are in folder order; the label lists of
  the sync need no order. Known limit with its own issue: each such
  label list of a full resync or a verify reads the whole mailbox.
- `labels` gives the system labels, the folder labels of section 6 and
  one label for each category of the master list
  (`/me/outlook/masterCategories`, page by page). Known limit with its own issue: a
  category that a message has, but the master list does not, is not
  mirrored.
- `minimal` and `raw` read the message by ID for its labels, its thread
  and its time. `raw` then streams `$value`. The time is
  `receivedDateTime`, else `createdDateTime` (a draft can have no
  received time). A message with neither gives `Error::Decode`: the sync
  records a failure of that message, never a false date.
- One `Outlook` value serves one mailbox: it keeps the IDs of the
  well-known folders after the first read.
- `SENT` is never given (section 6). R6-16 decides how the copies that
  Iris sent get it.
- Section 5.1 has the state writes (R6-15) and section 5.2 the drafts,
  the send and `metadata` (R6-17).

### 4. Raw message

`raw(token, id, sink)` calls `GET /me/messages/{id}/$value` and copies the
body to the sink through a buffer of 64 KiB. The body is the MIME text of
the message (A4). The client never holds the full message. The client
counts the bytes and stops with `Error::Decode` above 150 MiB. This limit
is a choice of the client, above the largest message that Iris expects.

Graph builds this MIME text from its own store (A4). Thus the bytes of one
message can change between two fetches with no change of the mail. The
mirror compares the content, not the hash of the raw bytes, for an Outlook
account. R6-10 decides where this compare lives.

### 5. Write calls

| Call | Graph | Answer |
|---|---|---|
| `set_read(id, bool, etag)` | `PATCH /me/messages/{id}` `{"isRead"}`, with `If-Match: <etag>` when the etag is given | 200, or 412 |
| `set_flag(id, bool, etag)` | `PATCH` `{"flag":{"flagStatus":"flagged" or "notFlagged"}}`, with `If-Match: <etag>` when the etag is given | 200, or 412 |
| `set_categories(id, list, etag)` | `PATCH` `{"categories":[...]}`, the full list, with `If-Match: <etag>` | 200, or 412 |
| `move_to(id, folder, etag)` | `POST /me/messages/{id}/move` `{"destinationId"}` | 201, the message |
| `create_draft(mime)` | `POST /me/messages`, `text/plain`, base64 MIME | 201, the draft |
| `send_mime(mime)` | `POST /me/sendMail`, `text/plain`, base64 MIME | 202, no body |
| `sent_by_message_id(id)` | `GET /me/messages?$filter=internetMessageId eq '<id>'&$select=id,isDraft,sentDateTime,parentFolderId`, each page (a read) | 200, the matching messages |
| `draft_etag(id)` | `GET /me/messages/{id}?$select=id` | 200, the `@odata.etag` |

`set_categories` replaces the full list (A6). The caller reads the message
with its `@odata.etag` first, and sends the full new list with
`If-Match`. If the person changed the message after the read, Graph gives
HTTP 412 (A14): a definite rejection, and no category is lost. The caller
can read again and make a new PATCH, because the 412 call changed nothing.
The new PATCH applies the `label` or `unlabel` of the action to the
categories of the new read, and sends that new full list with the new
etag. It never sends the list of the first try, so a category that
another client added stays.

The send binds the approved bytes. Iris never sends a draft by its ID,
because the person or another client can change a draft at the server
after the approval. `send_mime` sends the stored raw bytes of the approved
draft (the same bytes that the approval hash covers, as for Gmail in spec
`2026-11-16-send.md`).

Graph builds the MIME text of a draft again at each read (A4), so a hash
of the server bytes cannot show a change. The release compares the
content that the text reads back to, not the bytes. The release of a
send does these steps:

1. `create_draft` gives the ID of the new draft. The draft row keeps it.
   Iris keeps no etag of the draft: the etag changes at each change of
   the message, also a flag, a category or a read by another client, so
   an etag check denies sends that changed nothing. The send carries the
   stored bytes, not the server draft, so the content check of step 2
   covers each field that the mail carries.
2. Before the send, `draft_raw` reads the server draft and its MIME
   text. The release runs the read back of the send spec section 1 on
   that text (the fields of the content hash). It goes on only if the
   draft exists and is a draft (`isDraft`), the `Message-ID` of the read
   back is the `Message-ID` that Iris set (the content hash does not
   cover it), and the content hash is the hash of the `draft` row. Else
   the person or another client
   changed it: the release denies the send ("The draft changed after the
   approval."), as spec 2 section 7.5 and the send spec require, and
   refreshes the `draft` row as the send spec section 4.1 says: the row
   gets the content, the hash and the exact bytes of the
   read back, or `foreign_at` when the text does not read back (for
   example, Graph changed its form). A new `send` action then binds to
   the content that the person wrote, or submit denies it. If Graph
   always gives a form that does not read back, each Outlook send is
   denied: the safe result, and the release gate shows it.
3. The warning check of spec `2026-11-17-send-checks.md`: the release
   computes the flags of the stored bytes again, and denies the send with
   `iris_changeset::new_flags` if any warning grew after the approval
   (for example `new_recipient`), as for Gmail.
4. The claim of spec `2026-11-16-send.md` section 5, step 3 (the ledger
   row in `writing` and the `ledger_send` row), then `send_mime` with the
   stored bytes. The answer 202 has no message ID, so the row stays in
   `writing`.
5. The completion finds the sent copy: `sent_by_message_id(id)` lists
   the messages of the whole mailbox (`/me/messages`, each folder) with
   `$filter=internetMessageId eq '<id>'` (A15), and follows each page, so
   a rule or a client that moved the copy does not hide it. A candidate
   counts only if its ID is not the ID of the Iris draft, `isDraft` is
   false and it has a `sentDateTime`: the original draft has the same
   `Message-ID` and content, and stays in Drafts, so it never counts. It
   reads the `$value` of the copy. It runs the content check of the send
   spec section 6 (the fields of the content hash, the headers that
   change who gets the message), and sets the ledger row `applied` or
   writes the error "Outlook sent other content than the approved
   content". If no copy is there yet, the row stays in `writing`, and
   each later recovery turn searches again. Recovery never calls
   `send_mime` again. If the search itself fails (an error or a refused
   filter), the result stays unknown: Iris never reads a failed search as
   "no copy". After 24 hours with no copy, the row fails with the result
   "unknown" kept and the claim kept, as for Gmail (issue #280): Iris
   does not say that the mail did not leave.

Iris does not delete the server draft after the send, because spec 2
says that Iris never deletes a draft, and a delete is a mailbox write
with no approval. The draft stays in Outlook Drafts. The outbox shows
"Sent. The draft stays in Outlook: delete it there." A known limit with
its own issue: the person can send the same draft a second time from
Outlook. A later card can make the delete an approved part of the send.

### 5.2 The drafts and the send of the provider (R6-17)

`iris_graph::Outlook` makes the draft and send calls of the provider
spec section 4 on these rules:

- `create_draft(raw)` is `POST /me/messages` with the MIME bytes. Graph
  puts a reply in its conversation from `In-Reply-To` and `References`,
  so the `thread` argument is not sent. The draft ID is the ID of the
  draft message.
- `drafts_with_id` and `messages_with_id` list `/me/messages` with
  `$filter=internetMessageId eq '<id>'` and follow each page (A15). The
  apply gives the ID with no angle brackets, and the provider adds them.
  `drafts_with_id` keeps only the messages with `isDraft` true.
- `draft_raw(id)` reads the message first. A message that is not a draft
  gives `NotFound`, as a sent Gmail draft. Then it streams `$value` and
  keeps at most `DRAFT_LIMIT` bytes. Above the limit the read stops, and
  `raw` is `None`.
- `send_draft(id, raw)` reads the `Message-ID` of `raw` first. No
  `Message-ID` is a definite rejection before any request. Then
  `sendMail` with `raw`. After the 202, the provider looks for the sent
  copy with `sent_copy`. No copy yet, or a failed search, gives
  `Error::Transport` (unknown): the claim stays, and the recovery looks
  again and never sends again.
- `sent_copy(message_id, draft)` is a call of the provider trait. Its
  default (Gmail) is the first message with the ID and the label `SENT`.
  The Outlook provider gives a message with the ID that is not the
  draft, not a draft, and has a `sentDateTime` (section 5, step 5).
- `metadata(id, names)` reads the message, then the start of `$value`
  up to the first empty line (at most 256 KiB), because Graph gives no
  raw headers of a sent message or a draft. The draft of a reply reads
  the headers of its parent this way. `mail-parser` reads the header
  block; each value is the raw value with its line folds removed, as
  Gmail gives it, and `Subject` is decoded (RFC 2047). Text that is not
  a header gives no header. A property test covers random input.
- The provider trait has `name()`: "Gmail", "Outlook", or "the mail
  server" for another provider. Each result of the send that the person
  reads names the provider, for example "the draft is gone at Outlook",
  "unknown: Outlook shows no sent message" and "Outlook sent other
  content than the approved content". Each error that ends with "sent
  other content than the approved content" stops each later release, of
  each provider.
- Iris never deletes the draft after the send. Known limit with its own
  issue (#634). The note in the outbox is part of R6-16.
- The recovery of a `create_draft` with an unknown result is the
  recovery of the apply for each provider: `drafts_with_id` finds the
  draft, or the row fails. Graph makes the draft in the call, so a draft
  that the search does not find does not come later. Evidence:
  `pending`. If a draft comes later, Outlook has one more draft and the
  person can delete it; nothing is sent.

### 5.1 The state writes of the provider (R6-15)

`iris_graph::Outlook` makes the state writes of the provider spec
section 4 on these rules:

- `modify(ids, add, remove)` reads each message first (one read by ID
  each) and plans its writes. Each refusal comes before any write of the
  call: a label that is not `INBOX`, `UNREAD`, `STARRED` or a
  `category:` label (a folder label, `TRASH`, `SPAM`, `DRAFT`, `SENT`),
  one label in `add` and `remove`, more than `BATCH_LIMIT` messages, and
  a change of `INBOX` on a message that is not in the inbox or the
  archive. The read state, the flag and the categories change in each
  folder (section 6). Then, for each message, in this order: the
  categories (the full new list from the read of this call), `isRead`,
  the flag, and the move last. Each write has `If-Match` on the newest
  etag of the message. A write that changes nothing is not sent.
- HTTP 412 on a write of a message means that another client changed the
  message after the read. The provider reads the message one more time,
  plans again from the new state, and writes once more, so that the
  change of the other client stays (A14, A17). A second error is the
  error of the call, as below.
- An error after a write of the call is `Error::Transport` (unknown), so
  the apply reads the labels and never calls again (provider spec
  section 6). An error of the first write is the error of that write.
- `trash` moves the message to `deleteditems`, only from the inbox or the
  archive. `untrash` moves it to the inbox, only from `deleteditems`.
  Each move has `If-Match` on the etag of the check read (A17).
- `create_label` gives a definite refusal: Iris makes no category,
  because the master list needs `MailboxSettings.ReadWrite` and a
  category that is not on the list is not mirrored. The person makes the
  category in Outlook first. Known limit with its own issue.
- `moves_folders()` of the provider trait is true for Outlook. After a
  successful `modify` that changes `INBOX` (a move) on such a provider, the apply
  reads the labels of each message (`minimal`) and writes them to the
  mirror, so that the mirror has one folder label before the next sync.
  If a read fails, the reads stop, and each message with no read keeps
  its complete labels from before the move until the next sync. The
  mirror never has a message with no folder label or with two. Gmail
  keeps the add and remove change of the call, with no read.
- Each write of `modify` has `If-Match` on the newest etag of the
  message, also `isRead` and the flag. Thus a message that another
  client moves after the read of the plan does not move: the 412 read
  plans again from the new folder, and refuses a move out of a user
  folder.

### 6. Map to the mirror

This mapping is a proposal for R6-10, which owns the provider interface:

| Graph | Mirror label |
|---|---|
| The folder of the message | exactly one folder label: `INBOX`, `TRASH` and `SPAM` for `inbox`, `deleteditems` and `junkemail`; for `drafts`, `DRAFT` only when `isDraft` is true, else `Folders/Drafts` (see below); `Folders/Archive`, `Folders/Outbox` and `Folders/Sent Items` for `archive`, `outbox` and `sentitems` (not `SENT`, see below); `Folders/<path>` for each other folder |
| Each category | one user label (see the names below) |
| `isRead` false | `UNREAD` |
| `flag.flagStatus` `flagged` | `STARRED` |
| `conversationId` | the thread ID |

Trust rules. A folder or a header in the MIME text is data that the
person or a sender can set. Two existing rules read such data as proof:

- `SENT` is proof that the owner wrote the message (sender axis, the
  hostile checks, the evidence of known people). The person can move a
  received message into Sent Items, so the folder is not proof. For an
  Outlook account, only a message that Iris sent itself (the sent copy of
  a ledger send row, section 5) gets `SENT`. Each other message in
  `sentitems` gets the label `Folders/Sent Items`, and the sender axis
  decides it from its headers as any received mail. Known limit with its
  own issue: mail that the owner sends from another Outlook client is not
  owner evidence, until the release gate confirms a provenance field
  (A16).
- `DRAFT` is also proof that the owner wrote the message. The person can
  move a received message into Drafts. `isDraft` is set by the server: it
  is true only for an item that was never sent, which only a client of
  the mailbox can make. For an Outlook account, `DRAFT` needs the folder
  `drafts` and `isDraft` true. A message in `drafts` with `isDraft` false
  gets `Folders/Drafts` and counts as received mail.
- `Authentication-Results`: the header parsers trust only the header of
  the Gmail receiver (`mx.google.com`). An Outlook message can carry a
  forged header with that authserv-id. For an Outlook account, Iris
  treats each `Authentication-Results` header as absent: the parsers get
  the provider of the account and trust no authserv-id for `outlook`.
  The DMARC result of each Outlook message is "unknown", so the sender
  axis and the link checks use their rules for mail with no result. Known
  limit with its own issue, until the release gate confirms which
  authserv-id Exchange Online writes and that it removes a forged one
  (A16).

Label names must stay distinct, because search, policy rules and label
actions match label names (spec 2). The names:

- `archive`, `outbox`, `sentitems` and a non-draft in `drafts`: the fixed
  names above, in each
  language of the mailbox.
- A folder that is not well-known: the label `Folders/<path>`, where the
  path is the display names from the top folder, joined with `/`. For
  example `Folders/Receipts/Projects`. In a display name, `%` becomes
  `%25`, `/` becomes `%2F` and `#` becomes `%23`, so that each `/` of
  the label is a level and no name has a `#`. Its provider ID is
  `folder:<id>`.
- If two or more folder labels (the fixed names included) are equal under
  the case folding of spec 2, each of them gets ` #` and the full folder
  ID at the end. Folder IDs are unique and no escaped name has a `#`, so
  the new names are unique.
- A category: the label with the category name, provider ID
  `category:<name>`. If the name is a system label name of spec 1 section
  4.10, or starts with `Folders/` or `Categories/`, the label is
  `Categories/<name>`. Each of these compares uses the Unicode case
  folding of the label lookup of spec 2, so `folders/x` also gets the
  prefix.
- If two or more category names of the account are equal under that case
  folding (for example `Travel` and `travel`), each of them gets the
  label `Categories/<name> #<hex>`, where `<hex>` is the hex of the UTF-8
  bytes of the exact name. Exact names differ, so the labels differ, and
  a label action by such a name writes the exact category.

Thus no category name gives a folder label or a system label, and two
sources never give one name. A label action (`label`, `unlabel`) on a
`Folders/` label gets a definite refusal before any write: a folder is
not a label, and Iris moves messages only with `archive`, `inbox` and
`trash`. A label action on a category label writes the category name
(with no `Categories/` prefix).

Archive in Iris is a move to the well-known `archive` folder. Inbox is a
move to `inbox`. Trash is a move to `deleteditems`.

The system labels keep their fixed provider IDs (`INBOX`, `DRAFT`,
`TRASH`, `SPAM`), so each caller that reads `provider_id = 'INBOX'` or
`'DRAFT'` works for Outlook mail as for Gmail. Each `Folders/` label has
the provider ID `folder:<folder id>`; `Folders/Drafts` is
`folder:<drafts id>`, distinct from `DRAFT`. The mirror knows the folder
of each message from its one folder label: a system ID names its
well-known folder (`INBOX` is `inbox`, `DRAFT` is `drafts`, `TRASH` is
`deleteditems`, `SPAM` is `junkemail`), and `folder:<id>` names that
folder. The move guard reads this, not the label name.
Iris moves a message with `archive`, `inbox` or `trash` only from
`inbox` or `archive`. Such a verb on a message in any other folder
(`deleteditems`, a user folder, `junkemail`, `sentitems`) gets a definite
refusal before any write. The undo of a `trash` (spec 2) is the only move
out of `deleteditems`, and it goes back to the folder that the ledger
`pre` labels name (`INBOX` for `inbox`, no `INBOX` for `archive`). Thus
the mirror labels of spec 2 stay inverse pairs: the undo of `archive` is
`inbox` and the reverse, both between the two folders, and a trash of an
archived message goes back to `archive`. The read state, the flag and the
categories are not moves, and work in each folder. R6-10 keeps the same rule for each folder provider.

### 7. Sign-in

`iris-auth` gets a Microsoft client next to the Google client:

- The user gives the application (client) ID of an own app registration
  (decision record `docs/decisions/2026-11-microsoft-sign-in.md`, in the
  sign-in PR). A public client has no secret.
- Endpoints: `https://login.microsoftonline.com/<tenant>/oauth2/v2.0/authorize`
  and `/token`, with the host fixed in the code, as for Google. The user
  gives `<tenant>` with the client ID: `common` (the default) or the
  tenant ID (a GUID) of the registration. Iris accepts only these two
  forms. The decision record tells the user which to give:
  - A registration with the account type "Any Entra ID Tenant + Personal
    Microsoft accounts" works with `common`, for an Outlook.com account
    and for a work or school account. The decision record recommends it.
  - A single-tenant registration works only with its own tenant ID, and
    only for the accounts of that tenant. Microsoft refuses it at
    `common`. A personal account cannot use it.
- Scopes: `offline_access https://graph.microsoft.com/Mail.ReadWrite
  https://graph.microsoft.com/Mail.Send
  https://graph.microsoft.com/MailboxSettings.Read
  https://graph.microsoft.com/User.Read`. `MailboxSettings.Read` reads
  the category list for the label list (section 3.1).
  The code exchange and each refresh that names a `scope` check that it has
  `Mail.ReadWrite`, `Mail.Send` and `MailboxSettings.Read` (A10). If one
  is missing, the
  sign-in fails with an error that names the missing permission, and the
  account does not start.
- PKCE with S256 and a random `state`.
- Redirect: `http://localhost:<port>/oauth/callback`. Microsoft ignores the
  port of a `localhost` redirect (A9). The listener binds `127.0.0.1`.
- A refresh gives a new refresh token (A11). The refresh returns it, and
  the caller seals it with the store secret box in place of the old one.
  Microsoft does not revoke the old token, so a crash between the refresh
  and the write loses nothing.
- `invalid_grant` on a refresh gives `Error::Revoked`: the account needs a
  new sign-in.
- The address of the account is `mail` of `GET /me`, else
  `userPrincipalName`.

## Options not chosen

- One shared Iris app registration, with its client ID in the code. A
  public client has no secret, so this is allowed, and setup is shorter.
  Not chosen now: Microsoft can block consent to an unverified multitenant
  app in a work tenant (A12), the project must own the registration, and
  the Google decision (path A) gives each user an own client. Keep the
  client ID a setting, so that a shared ID can come later with no code
  change.
- `mailFolders/delta` for the folder tree: the depth is not documented (A2).
- `$batch` requests: Graph runs at most 4 requests of a batch at the same
  time (A5), so a batch saves round trips but not time, and makes the
  errors of each part harder. The gate of 4 is enough.
- A Graph SDK crate: no maintained Rust SDK for Graph mail fits a sync
  client on `ureq`. The client needs about 10 endpoints. Checked:
  `graph-rs-sdk` (async, `reqwest` and `tokio`, a second HTTP stack and
  more memory). The JSON parse uses `serde_json`, the MIME parse stays in
  `iris-clean` (`mail-parser`).
- IMAP to Outlook.com: card R6-03 builds IMAP for other providers. Graph
  gives categories, flags and immutable IDs that IMAP does not.

## Failure list

Supported input: JSON answers of Graph v1.0 in the shape of the Learn pages,
and MIME text of `$value`. All other input fails closed. A read gives
`Error::Decode` and the mirror changes nothing. A write that Graph
accepted (its success status: 200 for a `PATCH`, 201 for a move or a
draft, 202 for `sendMail`) can have changed the mailbox, so any bad
answer to it gives an unknown result (`Error::Transport`), and the caller
reconciles with reads: not JSON, another shape, another state, above
16 MiB, or another 2xx status. The rows below are for reads.

| Input or event | Result | Test |
|---|---|---|
| An answer that is not JSON, or JSON in another shape | `Error::Decode`, no change | fake returns bad JSON |
| A JSON answer above 16 MiB | `Error::Decode` | client limit |
| A `nextLink` or `deltaLink` to another host | `Error::Decode`, no request | unit test |
| An HTTP 3xx answer with a `Location` | `Error::Api` with the status; no request to the `Location` | fake redirect |
| A `$value` body above 150 MiB | `Error::Decode`, the sink has a part: discard it | fake with a limit |
| The connection ends in a page or a body | `Error::Transport`; the caller calls the same link again | fake that cuts |
| The same message two times in one round | the last one wins | fake replays |
| A message moves during the round of a folder | `Removed` in one folder, `Message` in the other | fake move |
| A message moves after the round of its new folder | `Removed` only; the read by ID finds the new folder, and the message stays | fake move between rounds |
| A folder of the mirror is not in the folder list | its link goes; each of its messages is read by ID: 404 is gone, else its new folder | fake folder delete |
| An Outlook message with a forged `Authentication-Results: mx.google.com; dmarc=pass` | the header counts as absent; DMARC is unknown | unit test of the parser with the provider `outlook` |
| A search folder in the folder list | left out with its children; its messages come from their real folders | fake search folder |
| A hidden child folder | listed, its messages synced | fake hidden child |
| A received message moved into Drafts (`isDraft` false) | label `Folders/Drafts`, no `DRAFT` | fake move to `drafts` |
| A received message moved into Sent Items | label `Folders/Sent Items`, no `SENT`; the sender axis treats it as received mail | fake move to `sentitems` |
| An expired delta link | `Error::DeltaReset`, then `HistoryExpired`: a full resync of the account | fake expiry, `iris-sync` test `an_expired_link_starts_a_full_resync` |
| 429 or 503 with `Retry-After` on a read | wait for it, then call again; a write never calls again | fake with `Retry-After` |
| A crash in a round | the last stored link is the start; Graph replays changes, the mirror applies each one again with the same result | R6-10 part |

## Impact

- New crates `iris-graph` and `iris-fakegraph`. No change to the store, the
  sync or the API before R6-10 merges.
- `iris-auth`: a Microsoft client. The Google client does not change.
- Part B (after R6-10): the provider interface for Graph, the account kind
  `outlook`, the `iris` sign-in command for Microsoft, and the docs.

Memory: the client keeps one page of at most 200 changes and a 64 KiB copy
buffer. The PR of the client measures the peak with the fake.

## Assumptions

Each assumption has the evidence of a Microsoft Learn page, read on
2026-11-17. A wrong assumption fails closed as the design says.

- A1. Message delta: `@removed` with `reason: deleted` for a delete or a
  move out of the folder; replays and repeats can occur; an expired token
  gives a 4xx such as `syncStateNotFound`, or 410 with a `Location`.
  Evidence: <https://learn.microsoft.com/en-us/graph/delta-query-messages>,
  <https://learn.microsoft.com/en-us/graph/delta-query-overview>. Page size:
  `Prefer: odata.maxpagesize` is honoured, with no documented maximum; the
  client follows the links and does not count on a size.
- A2. `mailFolders/delta` depth: not documented. Evidence:
  <https://learn.microsoft.com/en-us/graph/api/mailfolder-delta>. The design
  does not use it.
- A3. An immutable ID stays the same on a move in the same mailbox, and
  delta honours the `Prefer` header. It changes on a move to the archive
  mailbox. Evidence: <https://learn.microsoft.com/en-us/graph/outlook-immutable-id>.
- A4. `$value` gives the MIME text, built by Graph. Evidence:
  <https://learn.microsoft.com/en-us/graph/outlook-get-mime-message>. Size
  limit and drafts: `pending`; the client has its own limit, and the build
  tests a draft against the real mailbox in the release gate.
- A5. Outlook limits for each app and mailbox: 4 requests at the same time,
  10,000 requests in 10 minutes, 150 MB upload in 5 minutes; 429 has
  `Retry-After`. Evidence: <https://learn.microsoft.com/en-us/graph/throttling-limits>,
  <https://learn.microsoft.com/en-us/graph/throttling>.
- A6. `PATCH` keeps the fields that the body does not name; a collection
  in the body replaces the full collection. Evidence:
  <https://learn.microsoft.com/en-us/graph/api/message-update>. The replace
  of the collection is `pending` (standard OData, not stated). Iris checks
  each answer: the 200 answer of the PATCH has the `categories` of the
  message, and the write is applied only if that list equals the
  requested list (as a set). Else Graph changed something, but not the
  request: the result is unknown, as an unknown result of section 5 (no
  second call). The mirror takes the list of the answer, the ledger keeps
  the row with the note "Outlook kept other categories", Iris tells the
  person, and Iris turns off category writes for the account (label and unlabel give a definite refusal)
  until a new release. The release gate tests an `unlabel`. R6-15: the
  Graph client gives `Error::Differs` for such an answer, and the provider
  turns off its category writes in memory, before any request. Card R6-16
  stores the switch with the account and shows the notice (limit #675).
- A7. Well-known folder names work in each language of the mailbox.
  Evidence: <https://learn.microsoft.com/en-us/graph/api/resources/mailfolder>.
- A8. Error body `{"error":{"code","message"}}`. Evidence:
  <https://learn.microsoft.com/en-us/graph/errors>.
- A9. A desktop app registers `http://localhost`; the port is ignored for a
  `localhost` redirect. Evidence:
  <https://learn.microsoft.com/en-us/entra/identity-platform/reply-url>.
- A10. The token answer has `scope`; the app compares it with the request.
  Evidence: <https://learn.microsoft.com/en-us/entra/identity-platform/scopes-oidc>.
- A11. Refresh tokens replace themselves at each use and the old one stays
  valid; the default life is 90 days. Evidence:
  <https://learn.microsoft.com/en-us/entra/identity-platform/refresh-tokens>.
- A12. Consent to an unverified multitenant app can be blocked in a work
  tenant with risk-based step-up consent. Evidence:
  <https://learn.microsoft.com/en-us/entra/identity-platform/publisher-verification-overview>.
- A14. A `PATCH` of a message with `If-Match` and an old `@odata.etag`
  gives HTTP 412, and a change of a draft changes its etag. Evidence: `pending` (OData v4 rule; the message-update
  page does not state it). The release gate tests it on the real mailbox.
  If Graph ignores `If-Match`, Iris turns off category writes for Outlook
  accounts (label and unlabel give a definite refusal), and the rest works.
  A draft edit that keeps its etag is still caught: the release also
  compares the content hash of the read back (section 5, step 2).
- A15. A list of `/me/messages` with `$filter=internetMessageId eq '<id>'`
  finds the sent copy in each folder, and the `$value` of the copy keeps
  the fields of the content hash. Evidence: `pending`; the release gate
  tests it. If Graph refuses the filter, each search fails, and each
  Outlook send stays unknown until a new release: the outbox tells the
  person, and no wrong result is marked good. If the content changes at
  Graph (for example a converted body), the completion writes the content
  error and each later release stops, as the send spec requires.
- A16. A provenance field that proves owner authorship of a message in
  Sent Items (for example the MAPI property `PidTagMessageFlags`), and the
  authserv-id that Exchange Online writes in `Authentication-Results`,
  with the removal of a forged header. Evidence: `pending`. Until the
  release gate confirms them, the trust rules of section 6 apply.
- A13. An app registration needs an Entra tenant; a person with only a
  personal Microsoft account makes one with a free Azure account. Evidence:
  <https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app>.
  Whether the tenant stays after the free trial: `pending`. The decision
  record tells dk to check it.
- A17. A `move` with `If-Match` and an old `@odata.etag` gives HTTP 412
  and moves nothing, so a message that another client moved or changed
  after the check read of Iris does not move (provider spec section 4).
  Evidence: `pending` (OData v4 rule; the move page does not state it).
  The release gate tests it. If Graph ignores `If-Match` on `move`, Iris
  turns off moves for Outlook accounts (`archive`, `inbox`, `trash` and
  `untrash` give a definite refusal before any write), and the rest
  works.

## External writes

Each write goes to the mailbox of the person through Graph. The changeset,
the ledger and the apply rules of spec 2 stay the same; R6-10 maps them.

| Call | Success | Definite rejection (4xx, not 429) | Unknown result (time-out, lost answer, 5xx, 429 of a send) | Retry |
|---|---|---|---|---|
No write is made two times after it started with an unknown result (spec 2
section 7.3). The caller reconciles with reads only.

| Call | Success | Definite rejection (4xx, not 429) | Unknown result (time-out, lost answer, 429, 5xx) | Retry |
|---|---|---|---|---|
| `set_read`, `set_flag` | the answer has the new state | `Error::Api`, the mirror does not change | the caller reads the message: the target state means applied; any other state keeps the result unknown, because another client can have reversed the write. Iris tells the person (spec 2 section 7.3) | never after an unknown result |
| `set_categories` | the answer has the new list | `Error::Api`; 412 means the message changed after the read: nothing changed | as `set_read` | after 412 only: one new read and one new PATCH |
| `move_to` | 201 with the moved message | `Error::Api` | the caller reads the message: the target folder means applied; any other folder keeps the result unknown, and Iris tells the person | never after an unknown result |
| `create_draft` | 201 with the draft ID | `Error::Api` | a draft can exist. The caller searches the drafts for its `Message-ID` header: found means created; not found means failed (section 5.2) | never after an unknown result |
| `send_mime` | 202: the row stays in `writing` until the completion finds and checks the sent copy (section 5) | `Error::Api`; the row fails and the claim goes, as in the send spec | the ledger keeps the claim; recovery runs the search of section 5, step 5 (each folder, no draft) and the completion (spec 2 section 7.5) | never |

## Open questions

- Does `$value` work for a draft, and what is its size limit? The fake
  follows the docs; the real check runs in the release gate.
- Does the browser on the Mac reach `127.0.0.1` for `http://localhost`?
  If not, the listener also binds `[::1]` on the same port.

## Acceptance criteria

1. The client against the fake lists the folder tree with child folders,
   runs the delta of each folder to the end, and fetches each raw message.
   A missing child folder, a missed page or a missed message makes it red.
2. A move in the fake gives `Removed` in one folder and `Message` in the
   other, with one ID. A new ID makes it red.
3. An expired link gives `Error::DeltaReset`. Any other error makes it red.
4. A link to another host sends no request. A request at the other host
   makes it red.
5. The fake counts requests at the same time; the most is 4. A fifth makes
   it red.
6. Each write makes one request also after 429 or 503. A second request
   makes it red. `send_mime` sends the given bytes; other bytes at the fake
   make it red.
7. `set_categories` with an old etag gets 412 from the fake and changes
   nothing. A changed list makes it red.
8. A Microsoft sign-in against a fake token endpoint gives a new refresh
   token on refresh. A missing `Mail.ReadWrite`, `Mail.Send` or
   `MailboxSettings.Read` scope gives an error.
9. After R6-10: an Outlook account syncs with zero differences against the
   fake, also after a move, a delete and an expired link.

## Verify

```bash
cargo test -p iris-graph -p iris-fakegraph -p iris-auth --locked
scripts/local-ci/run.sh
```

## Rollout

1. The cards of "Plan", in their order.
2. Release gate after R1-07: dk registers the app, signs in, and runs 7
   days on a real Outlook mailbox.

## Risks

- Graph builds the MIME text, so the raw bytes can change. The compare of
  content, not bytes, handles it (section 4).
- A work tenant can block consent (A12). Then that user needs an admin
  consent, or a shared verified app later.
- Rollback: the crates have no caller until part B. Remove the account kind
  `outlook` to turn the provider off.

## Plan

Each step is one card and one PR. The board has each card with its
`doneWhen` and `dependsOn`.

1. R6-04: this spec.
2. R6-11: the read calls of `iris-graph`, and `iris-fakegraph`. Depends
   on R6-04.
3. R6-12: the write calls of `iris-graph`, with the fake. Depends on
   R6-11.
4. R6-13: the Microsoft sign-in in `iris-auth`, and the decision record
   with the app registration steps for dk. Depends on R6-04.
5. R6-14: the sync side of the provider (section 3.1). Depends on
   R6-12, R6-13 and R6-10.
6. R6-15: the state writes of the provider (section 5.1). Depends on
   R6-14.
7. R6-17: the drafts and the send of the provider. Depends on R6-15.
8. R6-16: the engine wiring (account kind, sign-in, token storage,
   `Authentication-Results`, `SENT` of the copies that Iris sent).
   Depends on R6-17.
