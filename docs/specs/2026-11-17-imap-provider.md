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
other folders, and their copies would count each message two times.

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
| `DRAFT` | `DRAFT` | system | a folder with `\Drafts`, or the `\Draft` flag |
| `SPAM` | `SPAM` | system | a folder with `\Junk` |
| `TRASH` | `TRASH` | system | a folder with `\Trash` |
| A folder label | `folder:<name>` | system for a special-use folder, else user | each other folder, also `\Sent` and `\Archive` |
| A keyword label | `kw:<keyword>` | user | a keyword flag (no `\` at the start) |

No IMAP message gets `SENT`. `SENT` is proof that the owner wrote the
mail (the sender axis, the hostile checks, the known people). Anyone can
move a received message into the Sent folder. So a message in the `\Sent`
folder gets the system label `folder:<name>` only. The send card gives
`SENT` to the copies that Iris itself sent.

A message in the `\Archive` folder has the label `folder:<name>` and no
`INBOX`: the mirror shows it as archived.

`labels` gives the seven fixed labels that apply, each folder label, and a
keyword label for each keyword in the `PERMANENTFLAGS` and the `FLAGS` of
each folder. It gives no counts.

### 5. The cursor

The cursor is text with one line for each folder:
`<UIDVALIDITY> <UIDNEXT> <HIGHESTMODSEQ> <count> <folder>`. `profile` makes
it from an `EXAMINE` of each folder.

`changes(from, page)` gives one folder for each page. The page token is the
index of the folder in the list. For each folder of the list now:

| Case | Result |
|---|---|
| The folder is not in `from` | `Added` for each message of the folder |
| A new `UIDVALIDITY` | `HistoryExpired` |
| `HIGHESTMODSEQ` is the same and the count is the same | nothing |
| `QRESYNC` | `changes(since)`: `Added` (the full label set) for each changed message, `Deleted` for each `VANISHED` UID |
| `CONDSTORE` only | `changes(since)` as above. Removed mail: removed = count in `from` - (count now - the count of UIDs at or above the `UIDNEXT` of `from`). If removed is above 0, `HistoryExpired` |

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

- `profile`: the user as the address, counts 0 (the trait allows 0), the
  cursor of now.
- `messages(label, page)`: the page token is `<folder index>:<UID>`.
  Each page is up to 500 messages of one folder, from the highest UID down
  (newest first, as Gmail). With a folder label, `INBOX`, `DRAFT`, `SPAM`
  or `TRASH`, only that folder. With `UNREAD`, `STARRED` or a keyword
  label, `UID SEARCH` (`UNSEEN`, `FLAGGED`, `KEYWORD <k>`) in each folder.
- `minimal`, `metadata` and `raw`: `EXAMINE` of the folder of the ID, then
  a `UID FETCH`. A new `UIDVALIDITY`, or no message with the UID, is
  `NotFound`. `raw` gives the `INTERNALDATE` in milliseconds and the
  thread of section 3.
- `messages_with_id`: `UID SEARCH HEADER Message-ID <id>` in each folder.

### 7. Writes

| Call | Result |
|---|---|
| `modify` with only `UNREAD`, `STARRED` and `kw:` labels | `SELECT` of the folder of each ID, then `UID STORE <uid> +FLAGS.SILENT` or `-FLAGS.SILENT` |
| `modify` with any other label | `Api` 400 `notSupported`, before any write |
| `create_label(name)` | `kw:<name>` with no server call, if the name is a valid atom; else `Api` 400 `badName` |
| `trash`, `untrash` | `Api` 400 `notSupported` |
| drafts and send | `Api` 400 `notSupported` |

A `modify` checks all IDs and labels before the first write. It writes
each folder in turn. An error after the first `STORE` was sent is
`Transport` (unknown), as the provider spec (section 6) says for a
`modify` in parts. A `NO` to the first `STORE` with nothing sent before is
`Api` 400.

`UID STORE` with `.SILENT` changes only the named flags. A second apply of
the same flag change gives the same flags, but the apply never calls it
two times after an unknown result (spec 2 section 7.2).

### 8. The engine

- The account row has the column `provider` already (schema 1, today
  always `gmail`). An IMAP account has `imap`. A migration adds the
  columns `imap_host`, `imap_port` and `imap_user`. The password goes in
  the column `token` (a sealed secret, `Store::seal`, spec 1 section 5.4),
  as the Gmail refresh token does. It is never in a log.
- `iris account add-imap <address> --host <host> [--port 993] [--user
  <user>]` reads the password from the terminal with no echo, signs in
  once to check it, and saves the account.
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
| A cursor that does not parse | `HistoryExpired` | `bad_cursor` |
| A new `UIDVALIDITY` of a folder | `HistoryExpired` | `uidvalidity_change` |
| A folder deleted at the server | `HistoryExpired` | `folder_gone` |
| A removal with `CONDSTORE` only | `HistoryExpired` | `removal_condstore` |
| A server without `CONDSTORE` | `Api` 400 `noCondstore` | `no_condstore` |
| Header fields that do not parse | thread = the message ID | `thread_rules` |
| A `modify` with a label that moves mail | `Api` 400, nothing sent | `modify_refuses_moves` |
| A lost connection after the first `STORE` | `Transport` | `modify_unknown` |
| A wrong password | `Unauthorized` | `wrong_password` |

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
  can. `pending`: a folder with `NOMODSEQ` gets a full compare by
  `HistoryExpired` at each change, which is slow but correct.
- `UIDNEXT` comes with each `EXAMINE`. `pending`: without it, the
  provider uses the highest UID + 1 from a `UID FETCH *`.

## External writes

`UID STORE` of `\Seen`, `\Flagged` and keywords.

- Success: the server answers `OK`. The apply marks the action applied.
- Definite rejection: `NO` or `BAD` to the first `STORE`, before any other
  write. `Api` 400. The apply marks the action failed.
- Unknown: a lost connection or a time-out after a `STORE` was sent, or a
  `NO` after another `STORE` of the same call. `Transport`. The apply reads
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
3. A `\Sent` folder message has no `SENT` label.
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
