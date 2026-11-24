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

The owner decided (`docs/decisions.md`, entry D12):

- D4, option A: attachments yes, HTML no. The body stays plain text.
- D5, option A: a display name and `Bcc`. Send-as aliases come later,
  when someone asks.

## Goal

1. A draft can have 0 to 10 attachments. The approval of a send binds to
   the name, the type, the SHA-256 and the size of each attachment.
2. A draft can have `bcc` recipients. The approval binds to them, and the
   review shows them.
3. The `From` of each new draft has the display name of the owner, when
   Iris knows it.
4. Each rule of the send spec stays true: no second send, the read back,
   the hash check, the hold and the checks before the send.

## Not in scope

- HTML bodies (D4).
- Send-as aliases (D5). `From` stays the account address.
- An attachment from an agent (the MCP tool `draft`, a job session).
  An agent has no path to put a file into the store. A limit issue
  records this.
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
  - `name`: 1 to 255 characters, with no control character, no `/` and
    no `\`, and not `.` or `..`. The normal form has no space at the
    ends.
  - `type`: a MIME type `type/subtype` in lower case, 127 characters or
    fewer, with only the characters of an RFC 2045 token. The type
    `multipart/*` and `message/*` are refused, because a reader opens
    them as parts, not as a file.
  - `sha256`: the SHA-256 of the file, 64 lower case hexadecimal
    characters.
  - `size`: the size of the file in bytes, 1 or more.
  - The sum of the sizes is at most the limit of the provider of the
    account (section 5).

The content hash (send spec section 1) gets the fields `bcc` and
`attachments`, in the canonical JSON with sorted keys. An empty `bcc`
and an empty `attachments` are not in the JSON, so the hash of each
draft of an older build stays the same.

### 2. The attachment files

An attachment file is in the content folder of the store (spec 1,
section 5.3), at the path of its SHA-256, as a raw message is. The
folder is content addressed, so one file serves each draft that uses it.

- `POST /api/attachments` of the local API takes the bytes of one file
  as the request body, at most the largest limit of section 5. It writes
  the file with `Store::put_content` and gives
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
owner: the `From` name of the newest message in the mirror that is from
the account address and has the label `SENT`, from the `header` table.
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

| Provider | Most bytes of all attachments of one draft | Reason |
|---|---|---|
| Gmail | 10 MiB (10,485,760) | The JSON `raw` field of `drafts.create` and `drafts.send` holds the base64 text of the message (assumption 3). Iris holds a few copies of the message in memory during the apply. |
| Outlook | 1.5 MiB (1,572,864) | Graph takes the message as base64 text in one request of at most 4 MB (assumption 2). A body of 256 KiB and 1.5 MiB of files fit. |

The submit takes the limit of the provider of the account of the draft,
and denies a larger sum ("the attachments of a draft have at most N
bytes"). The API refuses a request body over 10 MiB with HTTP 413.

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
| A request body over 10 MiB to `POST /api/attachments` | HTTP 413, and no file | API test |
| A draft of an older build (no `bcc`, no `attachments`) | The same hash as before | unit test with a fixed hash |
| Any other input | The submit or the apply denies the action, and Iris sends nothing | |

## Impact

- `crates/iris-compose`: the content fields, the checks, `compose`,
  `read`, `read_sent` and `hash`.
- `crates/iris-changeset`: the limit of the provider at submit, the
  flags `new_recipient` and `attachment`.
- `crates/iris-apply`: the files and the display name at the apply of a
  `draft`; the `post` of the claim.
- `crates/iris-api`: `POST /api/attachments`; `bcc` and `attachments` in
  the draft view and the outbox.
- `crates/iris-mcp`: the text of the tool `draft`.
- `client/`: the write view, the review and the outbox.
- `docs/specs/2026-11-16-send.md`, `docs/spec/02-changeset-policy-ledger.md`
  section 4 and `docs/spec/schema/changeset.schema.json`.
- No migration.

## Assumptions

1. Gmail sends to each `Bcc` address of the `raw` message of
   `drafts.send` and removes the header from the copies of the other
   recipients. Evidence: Gmail API guide "Sending email"
   (developers.google.com/gmail/api/guides/sending) builds the message
   with its headers, and RFC 5322 section 3.6.3. `pending`: the owner
   checks it on the Mac (issue #302). If the sent copy keeps no `Bcc`,
   the sent check of section 4 takes the approved `bcc`. If Gmail does
   not send to `Bcc`, the owner sees it in the check, and the send
   switch stays off.
2. Graph `POST /me/messages` and `POST /me/sendMail` with MIME text take
   `Bcc` and attachments, and a request has at most 4 MB. Evidence:
   Microsoft Graph "Send Outlook messages from another user" and "Create
   message in MIME format" (learn.microsoft.com). `pending` on the
   owner's Mac. If Graph drops `Bcc` or a file from the draft, the read
   back gives other content, and the release denies the send: Iris sends
   nothing.
3. Gmail takes a `raw` message of 10 MiB of files in the JSON body of
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

None. No migration, no flag. The send switch stays off until the owner
check of #302.

## Risks

- A blind copy to a wrong address cannot be undone. The review shows
  each `bcc` address, and `new_recipient` marks a new one.
- A large file raises the memory of the apply for some seconds. The
  limits of section 5 bound it.
- Rollback: revert the code. A draft with `bcc` or attachments that a
  newer build made does not read back in an older build, so the older
  build denies its send (the safe result).

## Plan

1. Spec (this PR).
2. `iris-compose`: content, checks, compose, read back and hash.
3. Engine: submit limits, flags, apply (files and name), API upload and
   draft view, MCP text, open model.
4. Client: write view, review and outbox, with a before and after page.
