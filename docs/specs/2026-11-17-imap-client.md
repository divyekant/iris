# Spec: the IMAP client of the mirror

- Date: 2026-11-17
- Card: R6-03 (part A)
- Status: Draft

## Problem

Iris reads mail from Gmail only. Many people keep mail at a provider that
gives IMAP (RFC 3501 and RFC 9051) and no Gmail API: Fastmail, iCloud, a
company server, a self-hosted Dovecot. Release 6 adds providers. The
provider interface (R6-10) is in work in another thread. The IMAP client
does not depend on it, so it is built first.

## Goal

1. A crate `crates/iris-imap` gives a synchronous IMAP client that has all
   the reads that the mirror needs: sign-in, folders, the state of a
   folder, the changes since the last sync, a full list for a compare, and
   a streamed raw fetch.
2. The client uses `CONDSTORE` and `QRESYNC` (RFC 7162) when the server has
   them, and a full compare when it does not.
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
| `changes(since, each)` | `UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE <since>)`, plus `VANISHED` when `QRESYNC` is on | each changed message (UID, flags, mod-sequence) and each removed UID range |
| `list(from, to, each)` | `UID FETCH <from>:<to> (UID FLAGS)` | each message in the range |
| `fetch_raw(uid, sink)` | `UID FETCH <uid> (UID BODY.PEEK[])` | the RFC 5322 bytes, streamed to the sink |

- `EXAMINE` opens a folder read-only. The client never uses `SELECT`, so
  no read sets `\Seen` and no read changes the mailbox. `BODY.PEEK[]`
  does not set `\Seen` either.
- A folder name goes to the server as a quoted string. A name with a byte
  that a quoted string cannot hold (CR, LF, NUL, or a byte above 127) is
  `Error::Name`. Names from `LIST` use modified UTF-7, which is ASCII.
- `list` gives only messages inside the asked range. For `<from>:*`, a
  server gives the last message also when its UID is below `<from>`
  (RFC 3501 section 6.4.8). The client drops such a message.

### 3. Sync modes (for part B)

The client gives the reads. Part B does the sync with them, in this way:

| Server has | Changed flags and new mail | Removed mail |
|---|---|---|
| `QRESYNC` | `changes(since)` | the `VANISHED` ranges of `changes` |
| `CONDSTORE` only | `changes(since)` | a full UID compare with `list` |
| neither | a full compare with `list` | a full compare with `list` |

- The key of a mirror entry is (folder, `UIDVALIDITY`, UID). When the
  `UIDVALIDITY` of a folder changes, all UIDs of the folder are new: part B
  drops the entries of the folder and lists it again.
- The same message in two folders is two entries. Part B decides how the
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
RFC 3501, RFC 9051 and RFC 7162 give them, in the forms that `imap-proto`
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
2. `fetch_raw` of a 20 MiB message keeps the peak memory of the client
   under 2 MiB above its start. It goes red if the client buffers the
   message.
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
2. R6-03 part A: `crates/iris-fakeimap`, the test server.
3. R6-03 part A: `crates/iris-imap`, the client and its tests.
4. R6-03 part B (after R6-10): the provider trait implementation, the
   sealed password, the sync and the wiring.
