# Spec: what a send contains (attachments, Bcc, display name)

- Date: 2026-11-24
- Card: P3 of the product decisions of 2026-11-24 (D4, D5; issues #235
  and #236)
- Status: Draft

## Problem

A draft of Iris has a plain text body, `To` and `Cc` with bare addresses,
and a `From` with the account address only (send spec
`docs/specs/2026-11-16-send.md`, section 1). The person cannot attach a
file, cannot send a blind copy, and the recipients see an address with
no name. A mail with no name looks like spam.

The owner decided (`docs/decisions.md`, entry D12, items D4 and D5):

- D4, option A: attachments yes, HTML no. The body stays plain text.
- D5, option A: a display name and `Bcc`. Send-as aliases come later,
  when someone asks.

## Goal

1. A draft can have 0 to 10 attachments. The approval of a send binds to
   the name, the type, the SHA-256 and the size of each attachment.
2. A draft can have `bcc` recipients. The approval binds to them, and the
   review shows them.
3. The `From` of each new Gmail or Outlook draft has the display name of
   the owner, when Iris knows it.
4. Each rule of the send spec stays true: no second send, the read back,
   the hash check, the hold and the checks before the send.

## Not in scope

- HTML bodies (D4).
- Send-as aliases (D5). `From` stays the account address.
- An attachment from an agent (the MCP tool `draft`, a job session).
  Submit denies a draft with a file from an author that is not the
  person, so an agent cannot name a file of the folder by its hash. A
  limit issue records this.
- A forward of an attachment of a received message.
- A setting for the display name. Iris takes the name from the sent mail
  of the owner (section 3). A limit issue records this.
- Display names of the recipients. `To`, `Cc` and `Bcc` stay bare
  addresses.
- A cleanup of attachment files that no draft uses. A limit issue
  records this.

## Design

### 1. The draft content

The content of send spec section 1 gets two optional fields:

```json
{"to": ["priya@example.com"], "cc": [], "bcc": ["dk@example.org"],
 "subject": "Q4 numbers", "body": "Hi Priya,\n\nThe numbers are attached.\n",
 "attachments": [{"name": "q4.pdf", "type": "application/pdf",
   "sha256": "<64 hex>", "size": 48213}]}
```

- `bcc`: 0 to 50 addresses, with the address rules of `to` and `cc`. The
  sum of `to`, `cc` and `bcc` is 50 or fewer. An address in `to` or `cc`
  leaves `bcc`, as an address in `to` leaves `cc`. A draft still needs
  one or more addresses in `to`.
- `attachments`: 0 to 10 entries, in the order of the author. Each entry:
  - `name`: 1 to 255 characters, with no control character, no `/`, no
    `\` and no `=?`, and not `.` or `..`. A reader decodes `=?..?=` in a
    file name as an encoded word, so such a name does not read back the
    same (as a subject, #270). The normal form has no space at the ends.
  - `type`: a MIME type `type/subtype` in lower case, 127 characters or
    fewer, with only the characters of an RFC 2045 token. The type
    `multipart/*` and `message/*` are refused, because a reader opens
    them as parts, not as a file.
  - `sha256`: the SHA-256 of the file, 64 lower case hexadecimal
    characters.
  - `size`: the size of the file in bytes, 1 or more.
  - The bound of the size of the message (section 5) is at most the
    limit of the provider of the account.

The content hash (send spec section 1) gets the fields `bcc` and
`attachments`, in the canonical JSON with sorted keys. An empty `bcc`
and an empty `attachments` are not in the JSON, so the hash of each
draft of an older build stays the same.

### 2. The attachment files

An attachment file is in the attachment folder of the store,
`attachments/<aa>/<sha256>`, next to the content folder (spec 1, section
5.3). The folder is content addressed, so one file serves each draft
that uses it. It is a folder of its own, not the content folder, because
the content cleanup (spec 1, section 4.5) deletes a content file that no
message uses, and the backup copies only the content files of messages.

- The writer of the attachment folder is the writer of the content
  folder (`ContentWriter`: a temporary file, `fsync`, then a rename to
  the SHA-256), with another root.
- Nothing deletes a file of the attachment folder. The content cleanup
  reads only the content folder. A limit issue records the growth.
- `iris backup` copies each file of the attachment folder, with the
  check of its SHA-256, as it copies the content files. A restore then
  has each file that a draft action names.
- `POST /api/attachments` of the local API takes the bytes of one file
  as the request body, at most the largest limit of section 5. It writes
  the file to the attachment folder and gives
  `{"sha256": "...", "size": N}`. The client then puts the entry, with
  the name and the type, into the content of the draft.
- The submit of a `draft` action checks the entries (section 1). It does
  not read the files.
- The apply of a `draft` action reads each file. If a file is missing,
  or its SHA-256 or its size is not the value of its entry, the apply
  denies the action ("an attachment file is missing or changed"), and
  makes no call.

### 3. The display name

At the apply of a `draft` action, Iris takes the display name of the
owner from one of these sources:

- Outlook: `displayName` of `GET /me` at Graph (issue #864). The Outlook
  provider gives the label `SENT` only to a copy of a mail that Iris
  sent, so the mirror has no safe source of the name. The scope
  `User.Read` permits the call. A failed call fails the apply as each
  other failed read of the apply does.
- Gmail: the `From` name of the newest message in the mirror that is
  from the account address and has the label `SENT`, from the `header`
  table.
A name with a control character, or with more than 128 characters, is
not used. With no name, `From` is the address only, as before.

The name goes into the `post` of the ledger claim, so that recovery
makes the same bytes again (send spec, section 4, step 2). The name is
not in the content hash: send spec section 1 checks `From` by address
only, because the provider can set the name of the account itself.

### 4. The message

`iris-compose` makes the message (send spec, section 2) with these
changes:

- `From`: the display name and the address, or the address only.
- `Bcc`: the `bcc` addresses, when there are any. Gmail and Graph send
  to each `Bcc` address and do not show the header to the other
  recipients (assumptions 1 and 2).
- No attachment: one `text/plain; charset=utf-8` part, as before.
- One or more attachments: `multipart/mixed`, with the body as the first
  part (`text/plain; charset=utf-8`), then one part for each attachment,
  in order, with `Content-Disposition: attachment`, the file name and
  the type.

The read back (send spec, section 1) changes:

- `Bcc` is a header that Iris writes. It moves from the refused headers
  to the known headers. `Reply-To`, `Sender` and each `Resent-` header
  stay refused.
- A message with attachments reads back only in the form of this
  section: a `multipart/mixed` root, a first part `text/plain`, then
  only parts with a file. A nested message, a nested multipart, an
  HTML part, or a part with no file name gives no content. The read back
  computes the SHA-256 and the size of each decoded file, and takes the
  name and the type of the part.
- The message that Iris writes again for the content read back
  (send spec, section 1) uses the decoded files, so the compare of the
  headers stays the same.

The read back of a sent copy (send spec, section 6) reads `Bcc` the same
way. If the sent copy has no `Bcc` header, it takes the `bcc` of the
approved content, because a provider can remove the header from the sent
copy (assumption 1). A `Bcc` header with other addresses gives other
content.

A person who adds an attachment or a `Bcc` to an Iris draft at Gmail
now gets a draft that reads back, when the draft has the form of this
section. The engine refreshes the draft row (send spec, section 4.1),
and a new `send` action binds to the new content. The files of such a
draft are not in the content folder. The send does not need them: the
release sends the `raw` bytes of the draft row.

### 5. Size limits

The release reads the draft back at the provider (send spec, section 5,
step 2). `Provider::draft_raw` keeps at most `DRAFT_LIMIT` (4 MiB) of a
draft: a larger draft gives no raw message, and the release denies the
send. So the whole message, not only the files, must fit the limit.

| Provider | Most bytes of the message | Reason |
|---|---|---|
| Gmail | 4 MiB (4,194,304), `DRAFT_LIMIT` | The read back of the release keeps at most this size. |
| Outlook | 2,900,000 | Graph takes the message as base64 text in one request of at most 4 MB (assumption 2). 2,900,000 bytes give at most 3,866,668 characters of base64. |

The submit computes an upper bound of the size of the message that
compose writes, and denies a draft whose bound is over the limit of the
provider ("the mail is too large for <provider>: remove a file"). The
bound is:

- 64 KiB for the headers of the message and of each part, apart from
  `In-Reply-To` and `References`,
- three bytes for each byte of the body (quoted-printable at most
  triples a byte),
- for each file, 78 bytes for each 57 bytes of the file, rounded up
  (base64 lines of 76 characters and CRLF).

Compose never writes more than the bound plus the size of the reply
headers. The reply headers come from the parent at the apply (send spec,
section 3), so the apply checks the size of the bytes that compose made,
and denies a larger message with the same reason, before the claim. With a body of 256 KiB, the
files of a Gmail draft can have about 2.2 MiB, and of an Outlook draft
about 1.4 MiB. The memory of the apply holds a few copies of a message
of 4 MiB at most.

The API refuses a request body over 4 MiB with HTTP 413.

### 6. Flags and the review

- `new_recipient` (send-checks spec) also reads `bcc`.
- `attachment` ("no file attached") marks a word of the attachment group
  only when the draft has no attachment.
- The draft view of the API (`GET /api/changesets/{id}/drafts`) and the
  outbox give `bcc` (with the same parts as `cc`) and `attachments`
  (name, type and size). The review and the outbox show both. A person
  never approves a send with a recipient or a file that the review did
  not show.
- The write view gets a `Bcc` field and a file button. Each change of
  the write view gets a before and after page and the owner's pick.

### 6.1 `POST /api/write`

The body of the press of Send (write spec
`docs/specs/2026-11-17-write.md`, section 2) gets two optional fields,
for a new mail and for a reply:

| Field | Rule |
|---|---|
| `bcc` | A list of addresses (section 1). Can be empty or absent. |
| `attachments` | A list of entries (section 1). Can be empty or absent. |

The engine puts each field that is not empty into the `args` of the
`draft` action. The `request_hash` of the send mark is the SHA-256 of
the target, the parent and the args, so it covers `bcc` and each entry
of `attachments`. A second press with the same key and another `bcc` or
another file gets 409 "This key belongs to another mail.". The client
makes a new key for each change of the form content (write spec,
section 6), and a change of `Bcc` or of the files is a change of the
form content.

### 6.2 What Iris can check of a `Bcc`

Iris reads the copy of the sender only. It cannot see if a `Bcc`
address got the mail, or if the copy of a `To` address shows the `Bcc`
header. So each provider gets a `Bcc` gate of its own, apart from the
send switch:

- A new table holds the `Bcc` gate of each provider (migration: the
  next free number on `develop` at the build):

  ```sql
  CREATE TABLE bcc_check (
    provider TEXT PRIMARY KEY,   -- 'gmail' or 'outlook'
    test_to TEXT NOT NULL,       -- the To address of the test send
    test_bcc TEXT NOT NULL,      -- the Bcc address of the test send
    checked_at INTEGER           -- NULL until the owner opens the gate
  );
  ```

  The migration adds no row. So a store whose send switch is already on
  still has no `Bcc` gate open.
- The owner check (issue #302 for Gmail, issue #649 for Outlook) sends
  mail made by Iris through the API path of the provider, because a
  test from the Gmail or Outlook app does not test that path:
  1. Run `iris sending bcc-test <gmail|outlook> <to> <bcc>`, with two
     addresses that the owner reads, at other mailboxes than the
     account (so that the provider does not merge the copies). The
     command writes the row with `checked_at` NULL. A new test replaces
     the addresses of a row that is not open.
  2. Write and approve a send in Iris with exactly `To` = `<to>`, no
     `Cc`, and `Bcc` = `<bcc>`, from an account of the provider. While
     the gate is closed, only a send with exactly these recipients
     passes it.
  3. Confirm that the `<bcc>` mailbox got the mail, and that the copy in
     the `<to>` mailbox has no `Bcc` header and does not name `<bcc>`.
  4. Run `iris sending bcc <gmail|outlook>`. It sets `checked_at`.
  Both commands need a terminal and a typed `yes`, as `iris sending on`,
  and open the store directly.
- The release of a send whose draft content has `bcc` reads the row of
  the provider of the account, in the transaction of the claim. If the
  row has no `checked_at`, and the send is not the test send of step 2,
  the claim does not happen: the send stays `held`, the release
  gives the outcome "waits for the Bcc check", and the outbox card says
  "Waits for the Bcc check of Gmail." (or Outlook). A held send expires
  14 days after its submit, as each held send does. A send with no `bcc`
  does not read the gate.
- No command removes a row. The owner turns all sends off with
  `iris sending off`.
- If the sent copy has no `Bcc` header, the completion takes the
  approved `bcc` (section 4), and the ledger entry gets the note "The
  sent copy shows no Bcc. Iris cannot confirm the blind copies." The
  ledger view shows the note.

### 7. The open model

`changeset.schema.json`: the `args` of `draft` get `bcc` (an array of
strings) and `attachments` (an array of objects with the four fields,
`additionalProperties` false). Spec 2, section 4, gets the same fields.
The MCP tool `draft` names `bcc` in its text, and not `attachments`
(Not in scope).

## Options not chosen

- The bytes of a file in the JSON of the content (base64): each copy of
  the content (the action `args`, the ledger `post`, the draft
  `content`, the API answers) then holds the file. The content folder
  holds one copy.
- A new attachment table with a migration: the content folder is already
  content addressed, and the content JSON holds the entries. No stored
  row changes.
- A display name setting: the sent mail of the owner already has the
  name, and a setting adds a screen and an API. A limit issue records
  the setting for later.
- Libraries: `mail-builder` (already in use) writes the multipart
  message and encodes the file names (RFC 2231). `mail-parser` (already
  in use) reads the parts and decodes the files. No new library.

## Failure list

Supported input: the content of section 1, files that the API wrote,
the drafts that the person changes at Gmail.

| Input or event | Result | Test |
|---|---|---|
| A `bcc` address with a space, CR, LF or another bad character | Submit denies the action | property test of the address check |
| `to`, `cc` and `bcc` with more than 50 addresses together | Submit denies the action | unit test |
| An address in `to` and in `bcc` | The normal form keeps it in `to` only | unit test |
| A file name with CR, LF, another control character, `/` or `\` | Submit denies the action | property test |
| A type that is not `type/subtype`, or `multipart/*` or `message/*` | Submit denies the action | unit test |
| 11 attachments, or a sum over the limit of the provider | Submit denies the action | unit test |
| Any content that the checks accept, with any files | Compose, then read back, gives the same content and hash, and no other header | property test |
| A file is missing or changed in the content folder at the apply | The apply denies the action, and makes no call | apply test |
| The person adds an HTML part, a nested message or a `Reply-To` at Gmail | No read back content: the release denies the action | unit test and fake Gmail test |
| The person adds an attachment or a `Bcc` at Gmail | The draft row gets the new content; a new send binds to it | fake Gmail test |
| The sent copy has no `Bcc` header | The completion takes the approved `bcc`, and the row becomes `applied` | unit test of the sent check |
| The sent copy has a `Bcc` header with another address | The error "Gmail sent other content than the approved content" | unit test of the sent check |
| A display name with a control character or over 128 characters | `From` is the address only | unit test |
| A crash after the claim of a draft with attachments | Recovery makes the same bytes from the post and the files | crash test |
| A backup, then a restore, with a draft action that names a file | The restore has the file; the apply sends | backup test |
| Two presses with the same key, the second with another `bcc` or file | 409 "This key belongs to another mail." | API test |
| A send with `bcc` while the provider has no open `bcc_check` row, also with the send switch on | The send stays `held`; the outbox says it waits for the Bcc check | release test |
| The test send of step 2, and a send with one more or one other recipient | The test send leaves; the other stays `held` | release test |
| A draft whose size bound is over the limit of the provider | Submit denies it ("the mail is too large ...") | unit test; property test that compose never passes the bound |
| The sent copy has no `Bcc` header | The ledger note "The sent copy shows no Bcc. Iris cannot confirm the blind copies." | unit test |
| A request body over 4 MiB to `POST /api/attachments` | HTTP 413, and no file | API test |
| A draft of an older build (no `bcc`, no `attachments`) | The same hash as before | unit test with a fixed hash |
| Any other input | The submit or the apply denies the action, and Iris sends nothing | |

## Impact

- `crates/iris-compose`: the content fields, the checks, `compose`,
  `read`, `read_sent` and `hash`.
- `crates/iris-changeset`: the limit of the provider at submit, the
  flags `new_recipient` and `attachment`.
- `crates/iris-apply`: the files and the display name at the apply of a
  `draft`; the `post` of the claim.
- `crates/iris-store`: the attachment folder, its writer and its copy in
  `iris backup`.
- `crates/iris-api`: `POST /api/attachments`; `bcc` and `attachments` in
  `POST /api/write`, the draft view and the outbox.
- `crates/iris-mcp`: the text of the tool `draft`.
- `client/`: the write view, the review and the outbox.
- `docs/specs/2026-11-16-send.md`, `docs/specs/2026-11-17-write.md`
  section 2, `docs/spec/02-changeset-policy-ledger.md`
  section 4 and `docs/spec/schema/changeset.schema.json`.
- `crates/iris-store`: one migration, the table `bcc_check`.
- `crates/iris`: the command `iris sending bcc <provider>`.

## Assumptions

1. Gmail sends to each `Bcc` address of the `raw` message of
   `drafts.send` and removes the header from the copies of the other
   recipients. Evidence: Gmail API guide "Sending email"
   (developers.google.com/gmail/api/guides/sending) builds the message
   with its headers, and RFC 5322 section 3.6.3. `pending`: the owner
   checks it on the Mac (section 6.2). If the sent copy keeps no `Bcc`,
   the sent check of section 4 takes the approved `bcc` and writes the
   note of section 6.2. If Gmail does not send to `Bcc`, or shows the
   header to a `To` address, the check of section 6.2 fails, and the send
   switch stays off.
2. Graph `POST /me/messages` and `POST /me/sendMail` with MIME text take
   `Bcc` and attachments, and a request has at most 4 MB. Evidence:
   Microsoft Graph "Send Outlook messages from another user" and "Create
   message in MIME format" (learn.microsoft.com). `pending` on the
   owner's Mac. If Graph drops `Bcc` or a file from the draft, the read
   back gives other content, and the release denies the send: Iris sends
   nothing. The recipient side is the check of section 6.2.
3. Gmail takes a `raw` message of 4 MiB in the JSON body of
   `drafts.create` and `drafts.send`. Evidence: Gmail API reference,
   `users.drafts.create` (the message size limit is 35 MB). `pending` on
   the owner's Mac. If Gmail refuses the size, the call is a definite
   rejection: the ledger row becomes `failed`, and Iris sends nothing.
4. `mail-builder` writes a file name with non-ASCII characters so that
   `mail-parser` reads the same name back. Evidence: the property test of
   the failure list.

## External writes

The calls `drafts.create` and `drafts.send` of Gmail, and the draft and
send calls of Graph, do not change. Send spec, section "External
writes", and the Outlook spec apply with no change. The message of the
call can now have a `Bcc` header and attachments.

## Open questions

- The exact form in which `mail-parser` gives a file name with RFC 2231
  continuation lines.

## Acceptance criteria

1. A draft with a `bcc` address and an attachment reads back, after the
   create at fake Gmail, with the approved hash, and the release sends
   it. Red if the read back drops `Bcc` or a file.
2. A change of a file of an approved draft at Gmail denies the send. Red
   if the hash does not cover the SHA-256 of each file.
3. The review shows each `bcc` address and each file name of a draft.
   Red if the draft view of the API leaves one out.
4. A new draft has `From: <name> <address>` when the sent mail of the
   owner has a name. Red if the apply does not read the name.
5. A draft of an older build keeps its hash. Red if an empty `bcc` or
   `attachments` goes into the canonical JSON.

## Verify

```bash
cargo test -p iris-compose -p iris-changeset -p iris-apply -p iris-api
scripts/local-ci/run.sh
```

## Rollout

1. The migration of `bcc_check` runs at the first start. It adds no row.
2. The send switch stays off until the owner check of #302. Each `Bcc`
   gate stays closed until the owner runs `iris sending bcc <provider>`.

## Risks

- A blind copy to a wrong address cannot be undone. The review shows
  each `bcc` address, and `new_recipient` marks a new one.
- A large file raises the memory of the apply for some seconds. The
  limits of section 5 bound it.
- Rollback: the migration goes forward only. An older program refuses a
  store with the new migration (`UnknownSchema`). Fix forward, or restore
  the backup copy that the store makes before each migration (spec 1,
  section 5.5). A draft with `bcc` or attachments does not read back in
  an older build, so the older build denies its send (the safe result).

## Plan

1. Spec (this PR).
2. `iris-compose`: content, checks, compose, read back and hash.
3. Engine: submit limits, flags, apply (files and name), the `Bcc` gate
   and its command, API upload and draft view, MCP text, open model.
4. Client: write view, review and outbox, with a before and after page.
