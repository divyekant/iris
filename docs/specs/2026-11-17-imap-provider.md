# Spec: the IMAP provider

- Date: 2026-11-17
- Card: R6-03 (part B)
- Status: Draft

## Problem

Part A (`docs/specs/2026-11-17-imap-client.md`) gives an IMAP client. The
sync, the verify and the apply call a mail provider only through
`iris_provider::Provider` (R6-10, `docs/specs/2026-11-17-provider.md`).
An IMAP account needs an implementation of that trait, and the engine must
choose it for the account.

## Goal

1. `iris_imap::Imap` implements `Provider`.
2. `iris_sync::sync` and `iris_sync::verify` run on a test IMAP mailbox
   with zero differences, with `QRESYNC` and with `CONDSTORE` only.
3. The apply can mark a message read, unread, starred or not starred, and
   add or remove a keyword label.
4. The engine keeps an IMAP account (host, port, user, sealed password)
   and syncs it with this provider.

## Not in scope

- Drafts and send. They need an SMTP client (RFC 6409) and the sent-copy
  setting of the provider spec, section 6. `create_draft`, `draft_raw`,
  `drafts_with_id` and `send_draft` give a definite rejection
  (`Error::Api` status 400, reason `notSupported`) until a later card.
- Moves (archive, inbox, trash, untrash, folder labels). The provider spec
  (section 4) says that a provider with location IDs refuses them.
- A server without `CONDSTORE`. `profile` gives `Error::Api` status 400,
  reason `noCondstore`, so the account cannot sync. A full compare at each
  sync would be the only way, and the sync would need the old UID set,
  which the cursor cannot hold.
- OAuth sign-in for IMAP (part A supports `XOAUTH2`, but there is no token
  source for it yet).

## Design

### 1. Connection

`Imap::new(server)` takes the host, the port, the user, and a time-out. The
`token` of each trait call is the app password. The provider keeps one
session behind a `Mutex<Option<Session<Tls>>>`. Each call takes the lock,
connects and signs in if there is no session, and drops the session after
any error. The calls of one provider run one at a time.

Errors map as follows:

| `iris_imap::Error` | `iris_provider::Error` |
|---|---|
| `SignIn` | `Unauthorized` |
| `Io`, `Closed`, `Tls` | `Transport` |
| `Protocol`, `TooLarge` | `Decode` (a read) |
| `Refused` | `Api` status 400 (a read: the folder is gone or the server refuses) |
| `Missing` | `Api` status 400, reason `missing` |
| `Name` | `Api` status 400, reason `badName` |

### 2. Folders

The provider reads the folder list (`LIST "" "*"`) at each `profile`,
`labels`, `messages` and `changes` call. It skips folders that cannot be
selected, and folders with `\All` or `\Flagged`: these are views of mail in
other folders, and their copies would count each message two times. Mail
that is only in a `\All` folder (Gmail over IMAP) does not sync: limit
issue #633.

The order of the folders is `INBOX` first, then the other folders by the
bytes of the name. A page token holds the name of its folder, not an
index, so a change of the LIST order between two calls does not skip a
folder. The next page is the first folder after that name in the order of
now. A folder that a page sequence skips because it was made during the
sequence is not in the cursor of `profile` or of the last `changes`, so
the next `changes` gives `Added` for each of its messages.

### 3. Message ID and thread ID

The message ID is a location ID: `<UIDVALIDITY>:<UID>:<folder>`. The folder
is the name as the server sends it (ASCII). A move at the server is a
delete and an add (provider spec, section 4).

The thread ID is the first `Message-ID` of `References`, else the
`In-Reply-To` value, else the own `Message-ID`, else the message ID. The
provider reads the three header fields with
`BODY.PEEK[HEADER.FIELDS (MESSAGE-ID IN-REPLY-TO REFERENCES)]` in each list
and in each raw read, so that the list and the raw read give the same
thread. Two copies of one message in two folders are two messages in one
thread.

### 4. Labels

| Iris label | ID | Kind | IMAP |
|---|---|---|---|
| `INBOX` | `INBOX` | system | the folder `INBOX` |
| `UNREAD` | `UNREAD` | system | no `\Seen` flag |
| `STARRED` | `STARRED` | system | the `\Flagged` flag |
| `SPAM` | `SPAM` | system | a folder with `\Junk` |
| `TRASH` | `TRASH` | system | a folder with `\Trash` |
| A folder label | `folder:<name>` | system for a special-use folder, else user | each other folder, also `\Sent`, `\Drafts` and `\Archive` |
| `UNSENT` | `UNSENT` | system | a folder with `\Drafts`, or the `\Draft` flag |
| A keyword label | `kw:<keyword>` | user | a keyword flag (no `\` at the start) |

No IMAP message gets `SENT` or `DRAFT`. Each of the two labels is proof
that the owner wrote the mail (the sender axis, the hostile checks, the
known people). Anyone can move or copy a received message into the Sent
or the Drafts folder, and any client can set the `\Draft` flag. So a
message in the `\Sent` or the `\Drafts` folder gets the system label
`folder:<name>` only. The send card gives `SENT` to the copies that Iris
itself sent.

A draft must still stay out of the understanding (spec 1 section 6.7).
So a message in the `\Drafts` folder or with the `\Draft` flag gets the
system label `UNSENT`. `UNSENT` is not proof of authorship: the sender
axis and the hostile checks ignore it. The understanding skips a message
with `UNSENT` as it skips one with `DRAFT`: the engine part of this card
adds `UNSENT` to each query of `crates/iris-understand` that excludes
`DRAFT` (`batch.rs` and the item query of `store.rs`). No IMAP account
can sync before that part, because only that part adds the account.

A message in the `\Archive` folder has the label `folder:<name>` and no
`INBOX`: the mirror shows it as archived.

`labels` gives the fixed labels `INBOX`, `UNREAD`, `STARRED`, `UNSENT`,
`SPAM` and `TRASH` that apply, each folder label, and a
keyword label for each keyword in the `PERMANENTFLAGS` and the `FLAGS` of
each folder. It gives no counts.

### 5. The cursor

The cursor is text with one line for each folder:
`<UIDVALIDITY> <UIDNEXT> <HIGHESTMODSEQ> <count> <folder>`. `profile` makes
it from an `EXAMINE` of each folder.

`changes(from, page)` gives one window of one folder for each page, from
the newest message down. A window is a range of sequence numbers, not of
UIDs: sequence numbers are dense (1 to `EXISTS`), so the page count follows
the message count, and a folder with large or sparse UIDs needs no empty
pages. A window is `FETCH <low>:<high> (UID FLAGS ...)`:

- for a folder that is not in `from`: 2,000 sequence numbers, so at most
  2,000 `Added` records;
- for a known folder: 20,000 sequence numbers with `(CHANGEDSINCE
  <modseq>)`, so at most 20,000 `Added` records, and only the changed
  messages come back.

A removal during the sequence moves the messages above it down by one
sequence number. The windows go from the top down, so a message can come
two times (the same `Added` two times gives the same mirror), but no
message is skipped. Mail that comes after the `EXAMINE` has a higher
sequence number and a higher mod-sequence: the next `changes` gives it.
Each page clamps its window to the `EXISTS` of now.

The removals of a known folder come after its last window, at most
20,000 for each folder, else `HistoryExpired`:

- `QRESYNC`: `UID FETCH 1:4294967295 (UID) (CHANGEDSINCE <modseq>
  VANISHED)`. The provider reads only the `VANISHED` ranges.
- `CONDSTORE` only: `UID SEARCH UID <UIDNEXT of from>:<UIDNEXT now - 1>`
  gives the new UIDs that are here now (see the table).

The page token holds the next window (its highest sequence number), the
cursor line of the folder of the page (from the `EXAMINE` before its first
window), and the cursor lines of the folders before it. The cursor line of
a folder joins them only after its last page.

For each folder of the list now:

| Case | Result |
|---|---|
| The folder is not in `from` | `Added` for each message of the folder |
| A new `UIDVALIDITY` | `HistoryExpired` |
| No `HIGHESTMODSEQ` (`NOMODSEQ`) now or in `from` | `HistoryExpired`, also when nothing else changed: a flag change leaves no other trace. The resync then gets `Api` 400 `noCondstore` from `profile` (section 6) |
| `HIGHESTMODSEQ`, `UIDNEXT` and the count are the same | nothing |
| `QRESYNC` | Windows with `CHANGEDSINCE`: `Added` (the full label set) for each changed message. Then `Deleted` for each `VANISHED` UID |
| `CONDSTORE` only | Windows as above. Then the search of the new UIDs: removed = count in `from` - (count now - the new UIDs here now). If removed is not 0, `HistoryExpired`. A removal after the `EXAMINE` makes it `HistoryExpired` too, which is safe. A UID from the `UIDNEXT` of `from` up to the `UIDNEXT` of now that is not here now came and went after `from` (the first sync can list it): `Deleted` |

A folder in `from` that is not in the list now gives `HistoryExpired`. The
last page carries the cursor of now, made from the `EXAMINE` results of
this call, each read before the `changes` of its folder (part A spec,
section 3: a change during the call comes again in the next call).

`Added` with the full label set replaces the label set of a message that
the mirror has (`iris-sync`, `Op::Set`), and fetches a message that the
mirror does not have.

`HistoryExpired` starts a full resync (spec 1 section 4.4). With
`CONDSTORE` only, each sync that follows a removal is a full resync. A
`QRESYNC` server needs none.

### 6. Reads

- `profile`: `Api` 400 `noCondstore` if the server has no `CONDSTORE` or
  if the `EXAMINE` of a folder gives no `HIGHESTMODSEQ` (limit issue #629).
  Else the address of the account, as the person gave it to
  `iris account add-imap` (section 8), not the IMAP user name. The sync
  and the apply compare it with the address of the account row. Counts 0
  (the trait allows 0), and the cursor of now.
- `messages(label, page)`: the page token is a bound and the folder name
  (section 2). Each page is one window of 2,000 sequence numbers of one
  folder, from the top down: newest first in each folder, `INBOX` first. The
  provider spec (`messages`) allows this order for a folder provider.
  With a folder label, `INBOX`, `SPAM` or `TRASH`, only that folder. With
  `UNREAD`, `STARRED`, `UNSENT` or a keyword label, each folder, and each
  window is `UID SEARCH <low>:<high> <key>` (`UNSEEN`, `FLAGGED`, `DRAFT`,
  `KEYWORD <k>`) on the sequence numbers of the window, then a `UID FETCH`
  of the found UIDs. So one answer holds at most 2,000 UIDs, under the
  1 MiB response limit of the client. For `UNSENT`, each message of a
  `\Drafts` folder counts with no search.
- `minimal` and `raw`: `EXAMINE` of the folder of the ID, then
  a `UID FETCH`. A new `UIDVALIDITY`, or no message with the UID, is
  `NotFound`. A refused `EXAMINE` of a folder that is not in the LIST now
  is `NotFound` too, so that a resync marks the old messages of a deleted
  folder gone (`iris-sync`, `check_unlisted`). A refused `EXAMINE` of a
  folder in the LIST stays `Api` 400. `raw` gives the `INTERNALDATE` in
  milliseconds and the thread of section 3.
- `metadata` and `messages_with_id`: `Api` 400 `notSupported`. Only the
  drafts and the send use them (provider spec, section 6), and they are
  not in scope.

### 7. Writes

| Call | Result |
|---|---|
| `modify` with only `UNREAD`, `STARRED` and `kw:` labels | `SELECT` of the folder of each ID, a check of its `UIDVALIDITY`, then `UID STORE <uids> +FLAGS` or `-FLAGS` (not `.SILENT`) |
| `modify` with any other label | `Api` 400 `notSupported`, before any write |
| `create_label(name)` | `Api` 400 `notSupported` until the flag writes. With the flag writes: `kw:<name>` if the name is a valid atom, else `Api` 400 `badName` |
| `trash`, `untrash` | `Api` 400 `notSupported` |
| drafts and send | `Api` 400 `notSupported` |

A `modify` checks all IDs and labels before the first write. It writes
each folder in turn. After the `SELECT` of a folder, it compares the
`UIDVALIDITY` of the folder with the one in each ID of that folder. A
different value before any `STORE` was sent is `NotFound`, and nothing is
written. A different value after a `STORE` of an earlier folder is
`Transport` (unknown), and nothing more is written. An error after the first `STORE` was sent is
`Transport` (unknown), as the provider spec (section 6) says for a
`modify` in parts. A `NO` or `BAD` to a `STORE` is `Transport` too: a
server can change some of the UIDs of one `STORE` and then refuse the
rest, so a refusal does not prove that no flag changed. The apply reads
the labels of each target (spec 2 section 7.2). Only a refusal before any
`STORE` was sent (an unknown folder, a new `UIDVALIDITY`) is definite.

`UID STORE` without `.SILENT` gives an untagged `FETCH` with the flags of
each message that it changed or matched. An `OK` counts as success only if
each target UID came back with the asked flags. A UID with no `FETCH`
(another client removed or moved the message) or with other flags is
`Transport` (unknown): the apply reads the labels again and does not call
again. `UID STORE` changes only the named flags. A second apply of the
same flag change gives the same flags, but the apply never calls it two
times after an unknown result (spec 2 section 7.2).

### 8. The engine

- The account row has the column `provider` already (schema 1, today
  always `gmail`). An IMAP account has `imap`. A migration adds the
  columns `imap_host`, `imap_port` and `imap_user`. The password goes in
  the column `token` (a sealed secret, `Store::seal`, spec 1 section 5.4),
  as the Gmail refresh token does. It is never in a log.
- `iris account add-imap <address> --host <host> [--port 993] [--user
  <user>]` reads the password from the terminal with no echo, signs in
  once to check it, and saves the account. If an account with the address
  exists, the command refuses it unless that account is `imap` with the
  same host, port and user: then it only replaces the sealed password.
  So an address never gets the mirror and the apply state of another
  mailbox. To move an address to another mailbox, the person removes the
  account first.
- The engine worker of an IMAP account uses `Imap`, with the password as
  the token. `Unauthorized` marks the account `needs_sign_in`, as for
  Gmail.
- The `Authentication-Results` parsers trust the server ID
  `mx.google.com` only for a Gmail account. For an IMAP account a header
  with that ID counts as absent, because any sender can write it.

## Options not chosen

- A stable ID from `EMAILID` (RFC 8474): an `EMAILID` alone cannot fetch a
  message, so each read would need a search in each folder.
- A cursor with the UID set of each folder: it would grow with the
  mailbox, and the store keeps the cursor as one value.
- `SENT` for the `\Sent` folder: refused by the trust rule above.
- The `THREAD` extension (RFC 5256): few servers have it, and it gives
  threads per folder only.

## Failure list

| Input | Result | Test |
|---|---|---|
| A message ID that does not parse | `NotFound` | `bad_id` |
| A message that came and went after the cursor | `Deleted` | `came_and_went_after_the_cursor` |
| A new `UIDVALIDITY` between the read and the `STORE` | `NotFound`, nothing sent | `modify_uidvalidity` |
| A cursor that does not parse | `HistoryExpired` | `bad_cursor` |
| A new `UIDVALIDITY` of a folder | `HistoryExpired` | `uidvalidity_change` |
| A folder deleted at the server | `HistoryExpired` | `folder_gone` |
| A removal with `CONDSTORE` only | `HistoryExpired` | `removal_condstore` |
| A server without `CONDSTORE` | `Api` 400 `noCondstore` | `no_condstore` |
| Header fields that do not parse | thread = the message ID | `thread_rules` |
| A `modify` with a label that moves mail | `Api` 400, nothing sent | `modify_refuses_moves` |
| A lost connection after the first `STORE` | `Transport` | `modify_unknown` |
| A wrong password | `Unauthorized` | `wrong_password` |
| A new folder with more than 2,000 messages | pages of at most 2,000 records | `changes_pages` |
| A LIST order change between two message pages | each folder once | `message_pages_follow_folder_names` |
| A `NO` to a `STORE` | `Transport` | `modify_refused` |
| A read of a message in a deleted folder | `NotFound` | `folder_gone` |
| A `STORE` `OK` with no `FETCH` for a target UID | `Transport` | `modify_unconfirmed` |
| `add-imap` of an address that another mailbox has | refused, nothing written | `add_imap_other_mailbox` |

## Impact

- `crates/iris-imap`: a new module `provider`, `UID SEARCH`, `SELECT`,
  `UID STORE`, and header fields in `list` and `changes`.
- `crates/iris-fakeimap`: `SELECT`, `UID STORE`, `UID SEARCH`,
  `PERMANENTFLAGS`, header fields.
- `crates/iris-store`: a migration with the account columns.
- `crates/iris`: the command and the provider choice in the engine.
- `crates/iris-senders` (or the parser of `Authentication-Results`): the
  provider of the account.

## Assumptions

- `iris-sync` treats `Added` of a message that it has as a full label set.
  Evidence: `crates/iris-sync/src/lib.rs`, `apply`, `Op::Set`.
- A server with `CONDSTORE` gives `HIGHESTMODSEQ` for each folder that it
  can. `pending`: a folder with `NOMODSEQ` stops the sync of the account
  with `noCondstore` (limit issue #629), so a wrong assumption gives a
  clear refusal, not a wrong mirror.
- `UIDNEXT` comes with each `EXAMINE`. `pending`: without it, the
  provider uses the highest UID + 1 from a `UID FETCH *`.

## External writes

`UID STORE` of `\Seen`, `\Flagged` and keywords.

- Success: the server answers `OK`, and each target UID came back in a
  `FETCH` with the asked flags. The apply marks the action applied.
- Definite rejection: a check before any `STORE` was sent fails (a
  label that moves mail, a new `UIDVALIDITY`). `Api` 400 or `NotFound`.
  The apply marks the action failed.
- Unknown: a lost connection or a time-out after a `STORE` was sent, a
  `NO` or `BAD` to any `STORE`, or a target UID that did not come back
  with the asked flags. `Transport`. The apply reads
  the labels and never calls again (spec 2 section 7.2).
- Retry: none. The provider never sends a `STORE` again.

## Open questions

- None.

## Acceptance criteria

1. The sync of a test IMAP mailbox (both `QRESYNC` and `CONDSTORE` only)
   gives zero differences in `verify` after random adds, flag changes and
   removals. It goes red if `changes` loses an event.
2. A `modify` of `UNREAD` changes `\Seen` at the test server. A `modify`
   of `INBOX` sends nothing.
3. A `\Sent` folder message has no `SENT` label, and a `\Drafts` folder
   message or a `\Draft` message has no `DRAFT` label.
4. A message of an IMAP account with `Authentication-Results:
   mx.google.com; dkim=pass` has no trusted authentication result.

## Verify

```bash
cargo test -p iris-imap -p iris-sync -p iris --locked
```

## Rollout

The migration adds three columns that can be empty. Each existing account
keeps `provider = 'gmail'`.

## Risks

- A server quirk can stop the sync of one IMAP account. The real-mailbox
  check on dk's Mac finds it.
- Rollback: an IMAP account cannot sync after a rollback. Gmail accounts
  do not change.

## Plan

1. This spec.
2. The read side: the provider, the client and fake additions, and the
   sync test.
3. The flag writes.
4. The engine: migration, command, provider choice, trust rules.
5. A later card: drafts and send over SMTP.
