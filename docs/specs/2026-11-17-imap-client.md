# Spec: the IMAP client of the mirror

- Date: 2026-11-17
- Card: R6-03 (part A)
- Status: Agreed

## Problem

Iris reads mail from Gmail only. Many people keep mail at a provider that
gives IMAP (RFC 3501) and no Gmail API: Fastmail, iCloud, a
company server, a self-hosted Dovecot. Release 6 adds providers. The
provider interface (R6-10) is in work in another thread. The IMAP client
does not depend on it, so it is built first.

## Goal

1. A crate `crates/iris-imap` gives a synchronous IMAP client that has all
   the reads that the mirror needs: sign-in, folders, the state of a
   folder, the changes since the last sync, a full list for a compare, and
   a streamed raw fetch.
2. The client uses `CONDSTORE` and `QRESYNC` (RFC 7162) when the server has
   them. The part B spec (`docs/specs/2026-11-17-imap-provider.md`)
   changes the plan for a server without them: see section 3.
3. A test IMAP server (`crates/iris-fakeimap`) runs in local CI with no
   network. A property test shows that a mirror kept with the client agrees
   with the server after random changes, in each of the three modes.
4. The client uses little memory: no list of all messages and no full
   message is in memory.

## Not in scope

- The provider trait implementation and the wiring in the engine (part B,
  after R6-10 merges).
- The storage of the password and the account rows. Part B seals the
  password with `Store::seal` (spec 1 section 5.4) and gives it to the
  client for each connection.
- Writes to the mailbox: `STORE`, `MOVE`, `COPY`, `APPEND`, `EXPUNGE` and
  send (SMTP). The apply of actions to IMAP is a later card.
- IMAP4rev2 (RFC 9051) and `UTF8=ACCEPT` (RFC 6855). The client speaks
  IMAP4rev1 and turns on neither, so the server sends folder names in
  modified UTF-7, which is ASCII (RFC 3501 section 5.1.3). The parser
  refuses a `CAPABILITY` list without `IMAP4rev1`, so such a server gives
  `Error::Protocol`.
- `STARTTLS` on port 143 and plain connections. The client connects with
  TLS from the first byte (port 993) only.
- `IDLE` and `NOTIFY` (push). The engine syncs on its timer.
- More than one connection for each account.

## Design

### 1. Connection and sign-in

- `connect(host, port, timeout)` opens a TCP connection and a TLS session
  with `rustls` (ring) and the `webpki-roots` certificates. These are the
  TLS parts that the program already has through `ureq`. A failed
  handshake or a failed certificate check is an error. There is no
  fallback to a plain connection.
- `Session::start(stream, auth)` reads the greeting, asks for the
  capabilities, signs in, asks for the capabilities again (they can change
  after sign-in), and sends `ENABLE QRESYNC` when the server has
  `QRESYNC`. `start` takes any `Read + Write` stream, so that the tests use
  a plain local stream. Only `connect` makes a stream for the engine.
- Sign-in with a password (an app password of the provider):
  `AUTHENTICATE PLAIN` when the server has `AUTH=PLAIN`, else `LOGIN` with
  quoted strings, unless the server has `LOGINDISABLED`.
- Sign-in with an OAuth access token: `AUTHENTICATE XOAUTH2`. If the
  server answers with an error challenge, the client sends an empty line,
  as the mechanism requires, and gets the `NO`.
- The password and the token are parameters. The crate stores neither.
  No `Debug` output and no error text holds them.
- A `NO` or `BAD` answer to the sign-in is `Error::SignIn`, so that the
  caller asks the person to sign in again.

### 2. Reads

All reads use UIDs, never message sequence numbers.

| Call | IMAP command | Result |
|---|---|---|
| `folders` | `LIST "" "*"` | name (as the server sends it), delimiter, special use (`\Sent`, `\Trash`, `\Drafts`, `\Junk`, `\Archive`, `\All`, `\Flagged`), and if it can be selected |
| `examine(folder)` | `EXAMINE "<folder>"`, with `(CONDSTORE)` when the server has it | `UIDVALIDITY`, `UIDNEXT`, the count, and `HIGHESTMODSEQ` or none |
| `changes(since, each)` | `UID FETCH 1:4294967295 (UID FLAGS INTERNALDATE RFC822.SIZE) (CHANGEDSINCE <since>)`, plus `VANISHED` when `QRESYNC` is on | each changed message (an entry) and each removed UID range |
| `list(from, to, each)` | `UID FETCH <from>:<to> (UID FLAGS INTERNALDATE RFC822.SIZE)` | each message in the range (an entry) |
| `fetch_raw(uid, sink)` | `UID FETCH <uid> (UID BODY.PEEK[])` | the RFC 5322 bytes, streamed to the sink |

- An entry has the UID, the flags, the mod-sequence (with `CONDSTORE`),
  the `INTERNALDATE` (the arrival time at the server, as the provider
  internal date of spec 1) and the `RFC822.SIZE`.
- IMAP has no thread ID in RFC 3501. Part B finds the thread of a message
  from its `Message-ID`, `In-Reply-To` and `References` headers in the raw
  bytes, as spec 1 does for a message with no provider thread. It can use
  `THREADID` (RFC 8474) later, where the server has it.
- `changes` asks for the UIDs `1:4294967295`, not `1:*`. `*` is the
  largest UID now, so a removed UID above it would get no `VANISHED`.
- No error holds server text. A server can echo the password or the token
  in any answer, also after the sign-in. `Error::Refused` holds only the
  first word of the command.
- `EXAMINE` opens a folder read-only. The client never uses `SELECT`, so
  no read sets `\Seen` and no read changes the mailbox. `BODY.PEEK[]`
  does not set `\Seen` either.
- A folder name goes to the server as a quoted string. A name with a byte
  that a quoted string cannot hold (CR, LF, NUL, or a byte above 127) is
  `Error::Name`. In IMAP4rev1 mode, names from `LIST` use modified UTF-7,
  which is ASCII, so only a server defect gives such a name.
- `folders` removes the quoted-string escapes (`\\` and `\"`) from a
  name, because the parser keeps them. A name in a literal has no escapes.
- `list` gives only messages inside the asked range. For `<from>:*`, a
  server gives the last message also when its UID is below `<from>`
  (RFC 3501 section 6.4.8). The client drops such a message.

### 3. Sync of a folder (for part B)

The part B spec (`docs/specs/2026-11-17-imap-provider.md`, section 5)
replaces this section. The changes: a server without `CONDSTORE` cannot
sync (limit issue #629), and a full compare is a full resync of the
mirror (`HistoryExpired`), because the provider cursor cannot hold the
UID set of a folder. Part B also reads its pages in windows of sequence
numbers (`Session::window`), not of UIDs, because UIDs can be sparse. The
text below is the first plan.

The client gives the reads. Part B does the sync with them. The mode is
chosen for each folder at each sync, from the `EXAMINE` result, not from
the capabilities alone:

| `EXAMINE` gives | Changed flags and new mail | Removed mail |
|---|---|---|
| `HIGHESTMODSEQ`, and `QRESYNC` is on | `changes(since)` | the `VANISHED` ranges of `changes` |
| `HIGHESTMODSEQ`, no `QRESYNC` | `changes(since)` | a full UID compare with `list` |
| no `HIGHESTMODSEQ` (no `CONDSTORE`, or `NOMODSEQ` for the folder) | a full compare with `list` | a full compare with `list` |

The checkpoint of a folder is (`UIDVALIDITY`, `HIGHESTMODSEQ`):

1. Part B reads the checkpoint from `EXAMINE`, before any `changes` or
   `list` call.
2. Part B saves the results of the calls.
3. Part B saves the checkpoint of step 1 only after all results of steps
   2 are saved. A change that comes during the calls has a mod-sequence
   above the checkpoint, so the next `changes(since)` gives it again.
   Applying a change two times gives the same mirror.
4. After a crash or an error before step 3, the old checkpoint stays.
   The next sync does the same calls again. With no saved checkpoint, the
   next sync does a full compare.
5. If `changes` gives `Error::TooLarge` (for example, a very long
   `VANISHED` list), part B does a full compare of the folder for this
   sync.

The key of a mirror entry is (folder, `UIDVALIDITY`, UID). When the
`UIDVALIDITY` of a folder changes, all old UIDs of the folder are invalid:

1. In one transaction, part B marks each entry of the old `UIDVALIDITY`
   gone, as for a removed message (spec 1: the item keeps its sources,
   and the raw mail stays for the retention time), and saves the new
   `UIDVALIDITY` with no `HIGHESTMODSEQ`.
2. Part B lists the folder as a new folder. A message that the reset
   brought back under a new UID is a new entry.
3. After a crash between the steps, the saved `UIDVALIDITY` is the new
   one with no checkpoint, so the next sync does the full list again.

A folder can go away (a delete or a rename at the provider). At each
sync, part B compares the folder list of a successful `folders` call with
the folders that it has:

1. A folder that is not in the list is gone. In one transaction, part B
   marks each entry of the folder gone (as above) and deletes the
   checkpoint of the folder. A renamed folder is a gone folder and a new
   folder.
2. If `folders` fails, part B changes nothing and the sync of the account
   stops with the error.
3. After a crash before the transaction, the next sync does the same
   compare again. The transaction is all or nothing, so a repeat gives the
   same result.

The same message in two folders is two entries. Part B decides how the
mirror shows one message in two folders (see Open questions).

### 4. Memory

- `fetch_raw` copies the literal of `BODY[]` from the socket to the sink
  in small blocks. The full message is never in memory.
- Each other response is read into one buffer of at most 1 MiB. A larger
  response is `Error::TooLarge`.
- `changes` and `list` give each message to a callback. They keep no list.

## Options not chosen

- The `imap` crate (3.0.0-alpha.15): an alpha. It reads a full message
  into memory, and its default TLS is `native-tls` (OpenSSL on Linux).
- `async-imap` (0.12): needs an async runtime. The mirror code is
  synchronous and runs on blocking threads.
- `imap-codec` (2.0.0-alpha.10): an alpha, and a full codec of all
  commands, which is more code in the program than the mirror needs.
- `imap-next`: a state machine for `tokio`.
- Chosen: `imap-proto` 0.17 (the parser that `async-imap` uses, with
  `CONDSTORE` and `QRESYNC` responses) for each response, and own code for
  the commands and the framing. The framing is own code because no
  library streams a literal to a sink.

## Failure list

The server is outside the control of Iris. Supported input: responses as
RFC 3501 and RFC 7162 give them, in the forms that `imap-proto`
parses. All other input gives an error, and the caller ends the session.

| Input | Result | Test |
|---|---|---|
| A response that does not parse | `Error::Protocol` | fuzz test |
| A response or a line above 1 MiB (other than the `BODY[]` literal) | `Error::TooLarge`, no allocation above the limit | `too_large` |
| No answer | the read time-out of the socket, then `Error::Io` | `stall` |
| `BYE` | `Error::Closed` | fuzz test |
| `NO` or `BAD` to the sign-in | `Error::SignIn` | `bad_password`, `bad_token` |
| `NO` or `BAD` to another command | `Error::Refused` | `examine_missing` |
| A `BODY[]` response of another UID in `fetch_raw` | `Error::Protocol`. The caller drops the sink. | fuzz test |
| A message below `<from>` in `list` | dropped | `list_range` |
| A server with no TLS | handshake error, no plain fallback | `no_plain` |
| A folder name with CR, LF, NUL or 8-bit bytes | `Error::Name`, nothing sent | `bad_name` |
| A server without `IMAP4rev1` | `Error::Protocol` (the parser refuses its `CAPABILITY` list) | `rev2_only` |
| A status response with a byte above 127 just after `]` (this input makes `imap-proto` 0.17 panic) | `Error::Protocol`, checked before the parser | fuzz test |
| A quoted folder name with escapes | the escapes removed | `folders_and_examine` |

A session after an error is not used again. The caller connects again.

## Impact

- New crate `crates/iris-imap`: dependencies `imap-proto` (new), `rustls`,
  `webpki-roots` and `base64` (in the lock file already).
- New test crate `crates/iris-fakeimap`. The program does not depend on
  it.
- No change to the store, the API, a schema or a screen.

## Assumptions

- `rustls` 0.23 with ring and `webpki-roots` 1.0 are in `Cargo.lock`.
  Evidence: `grep -A1 'name = "rustls"' Cargo.lock`.
- `imap-proto` 0.17 parses `HIGHESTMODSEQ`, `MODSEQ` and `VANISHED`.
  Evidence: `src/rfc4551.rs` and `src/rfc7162.rs` of the crate.
- Fastmail and Dovecot have `QRESYNC`; Gmail IMAP has `CONDSTORE` but no
  `QRESYNC`; some servers have neither. `pending`: the real-mailbox check
  on dk's Mac shows the capabilities. The design has a mode for each case,
  so a wrong guess changes only the speed.
- App-password providers accept `AUTHENTICATE PLAIN` over TLS. `pending`.
  If a server has no `AUTH=PLAIN`, the client uses `LOGIN`.

## External writes

None. The client opens folders with `EXAMINE` and fetches with
`BODY.PEEK[]`, so no call changes the mailbox.

## Open questions

- How the mirror shows one message that is in two folders (for example
  in Inbox and in a label folder of Gmail IMAP). Part B, with the R6-10
  trait, decides: one row for each copy, or one row with a folder set
  found by `Message-ID`.

## Acceptance criteria

1. A mirror that part B could keep with the client agrees with the test
   server after random adds, flag changes, removals and `UIDVALIDITY`
   changes, in each of the three modes. It goes red if `changes`, `list`
   or `VANISHED` lose an event.
2. `fetch_raw` of a 20 MiB message works, although each buffered response
   is at most 1 MiB. It goes red if the client buffers the message.
3. Random and cut server responses never make the client panic or hang.

## Verify

```bash
cargo test -p iris-imap -p iris-fakeimap --locked
```

## Rollout

None. Nothing calls the crate until part B.

## Risks

- A server quirk that `imap-proto` does not parse stops the sync of that
  account with `Error::Protocol`. The fix is a parser change upstream or a
  quirk in the framing. The real-mailbox check finds such quirks.
- Rollback: remove the two crates. Nothing else uses them.

## Plan

1. R6-03 part A: this spec.
2. R6-03 part A: `crates/iris-imap`, the client, and
   `crates/iris-fakeimap`, the test server, in one PR. The test server is
   test code, as `iris-fakegmail` is.
3. R6-03 part B (after R6-10): the provider trait implementation, the
   sealed password, the sync and the wiring.
