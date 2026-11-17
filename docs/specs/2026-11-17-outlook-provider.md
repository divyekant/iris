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
   calls (read state, flag, categories, move, draft, send and the delete of a sent draft).
2. A crate `crates/iris-fakegraph` holds a fake Graph server on top of the
   synthetic mailbox. CI tests the client against the fake. No test calls
   Microsoft.
3. `crates/iris-auth` signs in to a Microsoft account with the
   authorization code flow and PKCE, as a public client (no secret).
4. After the provider interface (card R6-10) merges, the Graph client
   implements that interface, and the engine syncs an Outlook account.

The card is done when an Outlook mailbox syncs with zero differences
against the fake server. The release gate is 7 days with zero differences
on a real Outlook mailbox on dk's Mac, after R1-07 ends.

## Not in scope

- Exchange Web Services, IMAP to Outlook and on-premises Exchange.
- Calendar, contacts and tasks.
- The Exchange archive mailbox. A move to the archive mailbox changes the
  immutable ID (assumption A3), so the mirror sees a delete.
- Category colors and the category list of the mailbox
  (`outlook/masterCategories`). That call needs one more scope
  (`MailboxSettings.Read`). The mirror reads the categories of each message.
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
Else the client gives `Error::Decode` and sends no request. Thus a changed
answer cannot send the token to another host.

Waits after a refusal:

- HTTP 429 and HTTP 5xx of a read: wait for `Retry-After` when the answer
  has it, else 1 second, then double up to 5 minutes, plus up to 50 percent
  at random (A5), and try again.
- A write makes one request only. The client never tries a write again,
  also not after 429 or 503. A time-out, a lost answer, 429 and 5xx of a
  write give an unknown result (`Error::write_result_unknown`), and the
  caller reconciles it with a read (see "External writes").
- HTTP 401: `Error::Unauthorized`. The caller gets a new access token one
  time.
- HTTP 404: `Error::NotFound`.
- Other status: `Error::Api { status, code, message }`, with `code` from the
  Graph error body `{"error":{"code","message"}}` (A8).

### 2. Folders

`folders(token)` reads `/me/mailFolders?includeHiddenFolders=true` and, for
each folder with `childFolderCount > 0`, its `childFolders`, page by page.
It gives each folder with its ID, parent ID, display name, the counts, and
the well-known name when the folder is one of `inbox`, `sentitems`,
`drafts`, `deleteditems`, `junkemail`, `archive` or `outbox` (A7). It finds
the well-known names with one call `GET /me/mailFolders/{name}` each.

The client does not use `mailFolders/delta`. Microsoft does not say if it
gives child folders at all depths (A2). A mailbox has tens of folders, so a
full read at each sync costs a few calls.

### 3. Delta of the messages of one folder

`delta(token, folder, link)`:

- With no link: `GET /me/mailFolders/{id}/messages/delta` with
  `$select=id,conversationId,parentFolderId,categories,isRead,flag,isDraft,receivedDateTime,internetMessageId`
  and `Prefer: odata.maxpagesize=200`.
- With a link: the stored `nextLink` or `deltaLink`, as it is.

A page gives a list of changes and one of two links:

- `More(nextLink)`: call it for the next page.
- `Done(deltaLink)`: the end of this round. Store it for the next sync.

A change is one of:

- `Message(m)`: the message is in the folder now, with the fields above.
  Graph can give the same message more than one time (A1). The last one
  in the page order is the current state.
- `Removed(id)`: an item with `@removed`. The message left the folder: a
  delete or a move (A1). The caller does not delete the message at once:
  a move gives `Removed` in one folder and `Message` in another, with the
  same immutable ID. The rounds of two folders are not one snapshot: if
  the round of the new folder ran before the move, only the `Removed` of
  the old folder shows the move. Thus, after the round of each folder,
  the caller reads each removed message that no round gave as `Message`
  with `message(id)` (`GET /me/messages/{id}?$select=...`): HTTP 404 means
  gone, and a message means a move, with its new folder. The message
  stays in the mirror until this read gives 404.

A token that Graph does not accept any more gives HTTP 410, or another 4xx
with a code such as `syncStateNotFound` (A1). The client gives
`Error::DeltaReset` for 410 and for each 4xx of a call with a stored link,
except 401, 403 and 429. The caller then starts that folder again with no
link: a full resync of one folder.

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
| `set_read(id, bool)` | `PATCH /me/messages/{id}` `{"isRead"}` | 200 |
| `set_flag(id, bool)` | `PATCH` `{"flag":{"flagStatus":"flagged" or "notFlagged"}}` | 200 |
| `set_categories(id, list, etag)` | `PATCH` `{"categories":[...]}`, the full list, with `If-Match: <etag>` | 200, or 412 |
| `move_to(id, folder)` | `POST /me/messages/{id}/move` `{"destinationId"}` | 201, the message |
| `create_draft(mime)` | `POST /me/messages`, `text/plain`, base64 MIME | 201, the draft |
| `send_mime(mime)` | `POST /me/sendMail`, `text/plain`, base64 MIME | 202, no body |
| `draft_etag(id)` | `GET /me/messages/{id}?$select=id` | 200, the `@odata.etag` |
| `delete_draft(id, etag)` | `DELETE /me/messages/{id}` with `If-Match: <etag>` | 204, or 412 |

`set_categories` replaces the full list (A6). The caller reads the message
with its `@odata.etag` first, and sends the full new list with
`If-Match`. If the person changed the message after the read, Graph gives
HTTP 412 (A14): a definite rejection, and no category is lost. The caller
can read again and make a new PATCH, because the 412 call changed nothing.

The send binds the approved bytes. Iris never sends a draft by its ID,
because the person or another client can change a draft at the server
after the approval. `send_mime` sends the stored raw bytes of the approved
draft (the same bytes that the approval hash covers, as for Gmail in spec
`2026-11-16-send.md`).

Graph builds the MIME text of a draft again at each read (A4), so a hash
of the server bytes cannot show a change. The etag shows it. The release
of a send does these steps:

1. `create_draft` keeps the `@odata.etag` of the new draft with the draft
   row.
2. Before the send, `draft_etag` reads the server draft. If the draft is
   gone (404), or its etag is not the kept etag, the person or another
   client changed it: the release denies the send ("The draft changed
   after the approval."), as spec 2 section 7.5 and the send spec require.
3. `send_mime` sends the stored bytes.
4. `delete_draft` deletes the server draft with `If-Match` and the kept
   etag, so that the person does not send it a second time from Outlook.
   A draft that changed after step 2 gives 412, and Iris leaves it in
   place. A failed delete leaves a draft and changes no sent mail.

If Graph ignores `If-Match` on `DELETE` (A14), Iris does not delete the
draft: it stays in Drafts, and the outbox tells the person.

### 6. Map to the mirror

This mapping is a proposal for R6-10, which owns the provider interface:

| Graph | Mirror label |
|---|---|
| The folder of the message | one label for the folder; `INBOX`, `SENT`, `DRAFT`, `TRASH` and `SPAM` for `inbox`, `sentitems`, `drafts`, `deleteditems` and `junkemail` |
| Each category | one user label with the category name |
| `isRead` false | `UNREAD` |
| `flag.flagStatus` `flagged` | `STARRED` |
| `conversationId` | the thread ID |

Archive in Iris is a move to the well-known `archive` folder. Inbox is a
move to `inbox`. Trash is a move to `deleteditems`.

### 7. Sign-in

`iris-auth` gets a Microsoft client next to the Google client:

- The user gives the application (client) ID of an own app registration
  (decision record `docs/decisions/2026-11-microsoft-sign-in.md`, in the
  sign-in PR). A public client has no secret.
- Endpoints: `https://login.microsoftonline.com/common/oauth2/v2.0/authorize`
  and `/token`, fixed in the code, as for Google.
- Scopes: `offline_access https://graph.microsoft.com/Mail.ReadWrite
  https://graph.microsoft.com/Mail.Send https://graph.microsoft.com/User.Read`.
  The code exchange and each refresh that names a `scope` check that it has
  both `Mail.ReadWrite` and `Mail.Send` (A10). If one is missing, the
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
and MIME text of `$value`. All other input fails closed: the call gives
`Error::Decode` and the mirror changes nothing.

| Input or event | Result | Test |
|---|---|---|
| An answer that is not JSON, or JSON in another shape | `Error::Decode`, no change | fake returns bad JSON |
| A JSON answer above 16 MiB | `Error::Decode` | client limit |
| A `nextLink` or `deltaLink` to another host | `Error::Decode`, no request | unit test |
| A `$value` body above 150 MiB | `Error::Decode`, the sink has a part: discard it | fake with a limit |
| The connection ends in a page or a body | `Error::Transport`; the caller calls the same link again | fake that cuts |
| The same message two times in one round | the last one wins | fake replays |
| A message moves during the round of a folder | `Removed` in one folder, `Message` in the other | fake move |
| A message moves after the round of its new folder | `Removed` only; the read by ID finds the new folder, and the message stays | fake move between rounds |
| An expired delta link | `Error::DeltaReset`, a full round of that folder | fake expiry |
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
  of the collection is `pending` (standard OData, not stated); the caller
  always sends the full list, which is correct under both readings.
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
- A14. A `PATCH` or a `DELETE` of a message with `If-Match` and an old
  `@odata.etag` gives HTTP 412, and a change of a draft changes its etag. Evidence: `pending` (OData v4 rule; the message-update
  page does not state it). The release gate tests it on the real mailbox.
  If Graph ignores `If-Match`, Iris turns off category writes for Outlook
  accounts (label and unlabel give a definite refusal), and the rest works.
- A13. An app registration needs an Entra tenant; a person with only a
  personal Microsoft account makes one with a free Azure account. Evidence:
  <https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app>.
  Whether the tenant stays after the free trial: `pending`. The decision
  record tells dk to check it.

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
| `create_draft` | 201 with the draft ID | `Error::Api` | a draft can exist. The caller searches `drafts` for its `Message-ID` header: found means created, not found means failed | never after an unknown result |
| `send_mime` | 202 | `Error::Api` | the ledger keeps the claim, and the caller looks in `sentitems` for the `Message-ID` (spec 2 section 7.5) | never |
| `delete_draft` | 204 | `Error::Api`; 404 means the draft is gone, which is the target; 412 means the draft changed, and Iris leaves it | the caller reads the draft: 404 means deleted, else the draft stays | never after an unknown result |

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
   token on refresh. A missing `Mail.ReadWrite` or `Mail.Send` scope gives
   an error.
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
5. R6-14: the provider implementation and the engine wiring. Depends on
   R6-12, R6-13 and R6-10.
