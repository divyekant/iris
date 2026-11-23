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
3. The apply can mark a message read or unread, and add or remove a
   keyword label. The changeset has no star verb, so `STARRED` is read
   only in this card. The provider still maps `STARRED` to `\Flagged` in
   `modify` (section 7), for a later star verb.
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

`Imap::new(address, user, connect)` takes the account address (from the
account row), the IMAP user, and a function that opens the TLS stream to
the host and the port with the time-out. The values do not change for the
life of the provider. `profile` returns the given address (section 6), so
an account with the address `name@example.com` and the IMAP user `name`
passes the address check of the sync. The `token` of each trait call is
the app password. The provider keeps one
session behind a `Mutex<Option<Session<Tls>>>`. Each call takes the lock,
connects and signs in if there is no session, and drops the session after
any error. The calls of one provider run one at a time.

Errors map as follows:

| `iris_imap::Error` | `iris_provider::Error` |
|---|---|
| `SignIn` | `Unauthorized` |
| `Io`, `Closed`, `Tls` | `Transport` |
| `Protocol`, `TooLarge` | `Decode` (a read). In `changes`, `TooLarge` is `HistoryExpired`: a removal history above the 1 MiB response limit (a long `VANISHED` list) starts a full resync, which reads in bounded windows |
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
the next `changes` gives `HistoryExpired` (section 5), and the full resync
reads its messages. Issue #664 (fixed here).

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
thread if the message has one of the three header fields. A message with
none of them gets its own location ID as thread, so its copies are in two
threads. This is part of limit issue #631 (threads from headers only).

### 4. Labels

| Iris label | ID | Kind | IMAP |
|---|---|---|---|
| `INBOX` | `INBOX` | system | the folder `INBOX` |
| `UNREAD` | `UNREAD` | system | no `\Seen` flag |
| `STARRED` | `STARRED` | system | the `\Flagged` flag |
| `SPAM` | `SPAM` | system | a folder with `\Junk` |
| `TRASH` | `TRASH` | system | a folder with `\Trash` |
| `SENT_COPY` | `SENT_COPY` | system | a folder with `\Sent` |
| A folder label | `folder:<name>` | system | each other folder, also `\Sent`, `\Drafts` and `\Archive` |
| `UNSENT` | `UNSENT` | system | a folder with `\Drafts`, or the `\Draft` flag |
| A keyword label | `kw:<keyword>` | user | a keyword flag (no `\` at the start) |

IMAP compares keywords with no case (RFC 3501 section 9, note 1). So the
ID of a keyword label is `kw:` and the keyword in lowercase ASCII, in
each folder: `$Todo` and `$todo` give the one label `kw:$todo`. The name
of the label is the first spelling in folder order (section 2). A search
for the label sends the lowercase keyword, and the server matches each
spelling. A flag write (section 7) sends the lowercase keyword too.

`SPAM` is each folder with `\Junk`, and `TRASH` is each folder with
`\Trash`. A server can list two such folders: both give the label, and
`messages` with the label pages through each of them.

A server does not have to send the special-use attributes (RFC 6154).
The fallback works for each role on its own. For each of `\Sent`,
`\Drafts`, `\Junk` and `\Trash` that no folder of the LIST has, each
folder with no special-use attribute whose last name part (after the last
hierarchy delimiter) is a common name of that role gets the role, with no
case. Other LIST attributes (`\HasNoChildren`, `\HasChildren`, `\Marked`
and so on) do not count. So on a server that marks `Sent` with `\Sent`
and lists `Spam` with only `\HasNoChildren`, `Spam` gets `\Junk`:

| Role | Common names |
|---|---|
| `\Sent` | `Sent`, `Sent Items`, `Sent Messages`, `Sent Mail` |
| `\Drafts` | `Drafts`, `Draft` |
| `\Junk` | `Junk`, `Spam`, `Junk E-mail`, `Junk Email`, `Bulk Mail` |
| `\Trash` | `Trash`, `Deleted Items`, `Deleted Messages`, `Bin` |

A folder with the attribute wins: if one folder has `\Junk`, a folder
named `Spam` with no attribute is an ordinary folder. The mapping errs to
the safe side: a folder named `Sent` gets `SENT_COPY`, so its mail stays
out of the understanding, and never gets `SENT`. A role folder with
another name (for example `Gesendet`) on a server with no attributes is
an ordinary folder: limit issue #648 (English names only).

No IMAP message gets `SENT` or `DRAFT`. Each of the two labels is proof
that the owner wrote the mail (the sender axis, the hostile checks, the
known people). Anyone can move or copy a received message into the Sent
or the Drafts folder, and any client can set the `\Draft` flag. So a
message in the `\Sent` or the `\Drafts` folder never gets `SENT` or
`DRAFT`. It gets its folder label `folder:<name>` and the folder-state
label of the next paragraph (`SENT_COPY` or `UNSENT`). The send card
gives `SENT` to the copies that Iris itself sent.

A draft and a sent copy must still stay out of the understanding of
received mail (spec 1 section 6.7). So a message in the `\Drafts` folder
or with the `\Draft` flag gets the system label `UNSENT`, and a message in
the `\Sent` folder gets the system label `SENT_COPY`. Neither label is
proof of authorship: the sender axis and the hostile checks ignore them,
and a sent copy is no evidence of a known person (limit issue, filed with
the code PR). The understanding skips a message with `UNSENT` or
`SENT_COPY` as it skips one with `DRAFT`. The same holds for each other
check of received mail. The engine part of this card adds one store
helper, `iris_store::draft_or_copy!`, the SQL condition "the label
`DRAFT`, `UNSENT` or `SENT_COPY`" (and `iris_store::DRAFT_OR_COPY` for a
check in Rust). Each query that skipped `DRAFT` as mail that is not
received mail uses it:

- the understanding (`crates/iris-understand`: the pending query and the
  thread rule of `batch.rs`, the thread context of `store.rs`, the guard
  of `store::understand`),
- the watches (`first_arrival` of `crates/iris-store/src/search.rs`), the
  notices by rule (`crates/iris-policy/src/notice.rs`) and the messages
  that Ask reads (`crates/iris-understand/src/ask.rs`),
- the received-mail evidence of the sender axis (the `replied` and the
  `service` rules, `received!` of `crates/iris-senders`),
- the conversations (`crates/iris-talk`) and the people
  (`crates/iris-people`),
- the reply parent and the conversation of the write view
  (`crates/iris-changeset`, `crates/iris-api/src/write.rs`), the thread
  context of the drafts in your voice (`crates/iris-voice`), and the
  replies that wake a waiting job (`crates/iris-workspace/src/wake.rs`).

A message that the understanding never read has no hostile marks, so the
model context, the write view and the agent must not read a sent copy
either.

The checks of authorship read `SENT` and `DRAFT` only, because a copy is
no proof: the `you` kind of the sender axis, the mail-from-you rule of the attention, your
addresses and voice samples, the link mark of your mail, and the mark of
a job that read mail you did not send. The copied-text flag of the
changeset counts a phrase of a sent copy as external text, the safe side
(more flags). The test `each_draft_read_is_classified` lists each name of
`DRAFT`, `UNSENT` or `SENT_COPY` in `crates/*/src`, with its reason, and
fails on a new one. The test `each_gate_skips_drafts_and_sent_copies`
fails on a file that reads `may_be_hostile!` (a gate of received mail)
with no `draft_or_copy!`, unless it is listed with its reason. A watch test covers a message that gets the `\Draft`
flag after its body was saved: no arrival notice. No IMAP account can
sync before that part, because only the next part adds the account.

A move at the server is a delete and an add (section 3), so a message
moved into the Drafts or the Sent folder is a new message with the label
from its first sync, before any understanding. Only a `\Draft` flag set
in place gives `UNSENT` to a message that the understanding read before.
Store migration 41 holds the rule at the label write (issue 677): a new
`DRAFT`, `UNSENT` or `SENT_COPY` label deletes the items of the message
that a new understanding would replace (the view `item_droppable`: not a
conversation, not pinned), and marks its thread for a new conversation.
The batch runner skips such a message, so no understanding run is
needed. Each change of `SENT` or a label of `iris_store::DRAFT_OR_COPY`
also clears `understood_at`.

Each folder label has the kind `system`, a folder that the person made
too: the provider refuses each folder change (section 7), and the
provider spec (section 4) gives `system` to each folder that a provider
cannot change as a user label. So a `label` action cannot target a
folder. A message in the `\Archive` folder has its folder label, no
`INBOX`, `SPAM` or `TRASH`, and the flag labels `UNREAD` and `STARRED` of
its flags: the mirror shows it as archived, as a Gmail message with no
`INBOX`.


`labels` gives the fixed labels `INBOX`, `UNREAD`, `STARRED`, `UNSENT`,
`SENT_COPY`, `SPAM` and `TRASH` that apply, each folder label, and a
keyword label for each keyword in the `PERMANENTFLAGS` and the `FLAGS` of
each folder. It opens each folder with `SELECT`, not `EXAMINE`: a server
sends the keywords of `PERMANENTFLAGS` only to a read-write open (RFC 3501
section 6.3.2). So a permanent keyword that is on no message is a label,
and a label action can use it with no `create_label`. It gives no counts.

Each label has one name, with no case, so a lookup by name finds one
label. A keyword can be named `INBOX`, a folder can be named `Spam` with
no `\Junk`, and two folders can be named `Work` and `work`. The fixed
names are `INBOX`, `UNREAD`, `STARRED`, `SPAM`, `TRASH`, `SENT`, `DRAFT`,
`UNSENT` and `SENT_COPY`. The rule:

1. The fixed names are taken first.
2. Then each folder label and each keyword label, in ID order (`folder:`
   before `kw:`, then by bytes), takes its own name if that name is free,
   with no case.
3. Else it takes `<name> (folder)` or `<name> (keyword)`, then
   `<name> (folder 2)`, `<name> (folder 3)` and so on, the first that is
   free. A name that a suffix makes counts as taken too.

So `folder:Work` is `Work` and `folder:work` is `work (folder)`. The rule
gives the same names for the same LIST and flags. The IDs do not change. The test `label_names_are_unique` covers both.

### 5. The cursor

A cursor with two lines for one folder is not a cursor of the provider:
`changes` gives `HistoryExpired` (issue #663). The test
`cursor_parsers_property` covers random cursor text.

The cursor is text with one line for each folder:
`<UIDVALIDITY> <UIDNEXT> <HIGHESTMODSEQ> <count> <role> <folder>`. `profile`
makes it from an `EXAMINE` of each folder. The role is the special use of
the folder of section 4, from its attribute or its common name: `S`
(`\Sent`), `D` (`\Drafts`), `J` (`\Junk`), `T` (`\Trash`) or `-`.

`changes(from, page)` gives one window of one folder for each page, from
the newest message down. A window is a range of sequence numbers, not of
UIDs: sequence numbers are dense (1 to `EXISTS`), so the page count follows
the message count, and a folder with large or sparse UIDs needs no empty
pages. A window is 20,000 sequence numbers, `FETCH <low>:<high> (UID
FLAGS ...) (CHANGEDSINCE <modseq>)`, so at most 20,000 `Added` records,
and only the changed messages come back. Each folder of a window is in
`from` (see the table below).

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
- `CONDSTORE` only: if the `UIDNEXT` of now is above the `UIDNEXT` of
  `from`, `UID SEARCH UID <UIDNEXT of from>:<UIDNEXT now - 1>` gives the
  new UIDs that are here now (see the table), and the provider keeps only
  the UIDs inside that range. Else the set of new UIDs is empty and no
  search is sent: a reversed range such as `101:100` would name UID 100.

The page token holds the next window (its highest sequence number), the
cursor line of the folder of the page (from the `EXAMINE` before its first
window), and the cursor lines of the folders before it. The cursor line of
a folder joins them only after its last page.

For each folder of the list now:

| Case | Result |
|---|---|
| The folder is not in `from` | `HistoryExpired`. The first sync can list mail of a folder that is not in its start cursor, and that mail can go before the first `changes`. `from` has no UID set of the folder, so only a full resync removes that mail. A new folder at the server starts a full resync too: limit issue #660 |
| A new `UIDVALIDITY` | `HistoryExpired` |
| A new role (for example, a new `\Junk` folder takes the role from a folder named `Spam`) | `HistoryExpired`: the labels of each message of the folder change, and the folder has no other trace of the change |
| No `HIGHESTMODSEQ` (`NOMODSEQ`) now or in `from` | `HistoryExpired`, also when nothing else changed: a flag change leaves no other trace. The resync then gets `Api` 400 `noCondstore` from `profile` (section 6) |
| `HIGHESTMODSEQ`, `UIDNEXT` and the count are the same | nothing |
| `QRESYNC` | Windows with `CHANGEDSINCE`: `Added` (the full label set) for each changed message. Then `Deleted` for each `VANISHED` UID |
| `CONDSTORE` only | Windows as above. Then the search of the new UIDs: removed = count in `from` - (count now - the new UIDs here now). If removed is not 0, `HistoryExpired`. A removal after the `EXAMINE` makes it `HistoryExpired` too, which is safe. A UID from the `UIDNEXT` of `from` up to the `UIDNEXT` of now that is not here now came and went after `from` (the first sync can list it): `Deleted`. UIDs can be sparse, so this range can be large: if the `UIDNEXT` of now minus the `UIDNEXT` of `from` is more than 20,000, `HistoryExpired`, before any search. So one call gives at most 20,000 such `Deleted` records |

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
  Else the address of the account row. IMAP gives no identity of a
  mailbox that Iris can trust (the user name is the person's own input
  too). So the binding of an account to its mailbox is the host, the port
  and the user of the account row: the engine makes the provider of an
  account only from these values, and `add-imap` never changes them for
  an existing account (section 8). The address check of the sync and the
  apply then always passes for IMAP, by design. If the person signs in to
  another mailbox with `--user`, the mirror is the mail of that mailbox
  under the given address: Iris cannot detect it, and the person can see
  it at once in the mail. Counts 0
  (the trait allows 0), and the cursor of now.
- `messages(label, page)`: the page token is a bound and the folder name
  (section 2). Each page is one window of 2,000 sequence numbers of one
  folder, from the top down: newest first in each folder, `INBOX` first. The
  provider spec (`messages`) allows this order for a folder provider.
  With a folder label or `INBOX`, only that folder. With `SPAM` or `TRASH`, each
  folder with `\Junk` or `\Trash`, in folder order. With
  `SENT_COPY`, only the `\Sent` folders, all of their messages. With
  `UNREAD`, `STARRED`, `UNSENT` or a keyword label, each folder, and each
  window is `UID SEARCH <low>:<high> <key>` (`UNSEEN`, `FLAGGED`, `DRAFT`,
  `KEYWORD <k>`) on the sequence numbers of the window. A bare set in a
  search is a set of sequence numbers, also in `UID SEARCH` (RFC 3501
  section 6.4.4, the `<sequence set>` key; only the key `UID <set>` names
  UIDs, and `UID` changes only the numbers of the answer to UIDs). The
  test `search_windows_use_sequence_numbers` covers a folder whose UIDs
  do not start at 1. then a `UID FETCH`
  of the found UIDs. So one answer holds at most 2,000 UIDs, under the
  1 MiB response limit of the client. For `UNSENT`, each message of a
  `\Drafts` folder counts with no search.
- The size of a page. The 1 MiB limit bounds each answer, not the records
  that a page keeps, and each ID holds the folder name. So:
  - A folder name is at most 255 bytes. A longer name of a selectable
    folder makes each call `Api` 400 `longFolderName` (limit issue #650);
    a skipped folder would lose its mail with no notice.
  - A thread ID from a header is at most 998 bytes (the line limit of RFC
    5322). A longer one is ignored, and the message gets its own location
    ID as thread.
  - A page of `messages` then holds at most 2,000 IDs of about 1.3 KB
    each, so less than 3 MB. Each fetched entry becomes its compact
    reference in the fetch callback, so no header block stays for the
    whole page (issue #662).
  - A page of `changes` keeps at most 16 MiB of records (IDs, thread IDs
    and labels), `Added` and `Deleted` records together. If a page has
    more, `changes` is `HistoryExpired`, and
    the full resync reads pages of `messages` and one message at a time.
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
| `modify` with only `UNREAD`, `STARRED` and `kw:` labels | First a check of each target folder: `SELECT`, its `UIDVALIDITY`, and its `PERMANENTFLAGS`. Then, for each folder, `SELECT`, the `UIDVALIDITY` again, and `UID STORE <uids> +FLAGS` or `-FLAGS` (not `.SILENT`) |
| `modify` with any other label | `Api` 400 `notSupported`, before any write |
| `create_label(name)` | `Api` 400 `notSupported`, also with the flag writes (limit issue #652). IMAP has no call that makes a keyword with no message: if the targets of the action are gone before the apply, a created label would have no `STORE` behind it, and the next `labels` would not list it. A label action can use a keyword that a folder lists in `FLAGS` or `PERMANENTFLAGS` (section 4). Its ID is the lowercase `kw:` ID, so the ledger and the undo use the same ID as `labels`, `minimal` and `changes` |
| `trash`, `untrash` | `Api` 400 `notSupported` |
| drafts and send | `Api` 400 `notSupported` |

A `modify` checks all IDs and labels before the first write. It then
does a check pass with no `STORE`: a `SELECT` of each target folder, a
compare of its `UIDVALIDITY` with each ID of the folder, and a check that
each flag to change is permanent in the folder. A flag is permanent if the
`PERMANENTFLAGS` of the `SELECT` holds it, or holds `\*` for a keyword
(RFC 3501 section 7.1). A server can show a flag that is not permanent in
the answer to a `STORE` and lose it at the end of the session, so Iris
could record an action that does not last. A flag that is not permanent,
or a `SELECT` with no `PERMANENTFLAGS`, is `Api` 400 `notPermanent`, and
nothing is written. Then it writes each folder in turn. After the `SELECT` of a folder, it compares the
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
  mailbox. The Gmail sign-in (`iris account add`, `save_sign_in`) gets
  the same rule: if the address has an account of another provider, it
  refuses before it changes the sealed token or any account state. The
  check of the provider and the write of the token run in one store
  transaction. To move an address to another mailbox, the person removes the
  account first.
- `iris account add-imap <address>` with no `--host` gives a new password
  to the IMAP account of the address, with its saved server, port and
  user. With no IMAP account for the address, it refuses. The sign-in help
  of the web client shows only this form, with the address quoted for the
  shell, so the help never copies a saved value into a command.
- The engine worker of an IMAP account uses `Imap`, with the password as
  the token. `Unauthorized` marks the account `needs_sign_in`, as for
  Gmail. `provider_for` in `crates/iris/src/engine.rs` picks the provider
  from the `provider` column. If it cannot make one (for example, an IMAP
  row with no host), the engine logs the account and continues with the
  next account.
- `Store::save_account_secret` does the provider and mailbox check and the
  token write in one transaction, for both sign-in commands.
- Test mode: if `IRIS_TEST_IMAP_PLAIN` is set, the connection is plain TCP
  with no TLS. It is allowed only for the host `127.0.0.1`, so the
  binary tests can use the fake server. Any other host is refused.
- The `Authentication-Results` parsers trust the server ID
  `mx.google.com` only for a Gmail account. For an IMAP account a header
  with that ID counts as absent, because any sender can write it.
  `iris_senders::head::trusted_authserv` gives the ID to trust from the
  account provider. Both parsers take it as an argument
  (`iris_understand::Head::parse` and the DKIM check of the sender axis),
  so no caller can read a result with no provider.

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
| No `UIDNEXT` with `CONDSTORE` only | `Api` 400 `noUidnext` | `no_uidnext` |
| Header fields that do not parse | thread = the message ID | `thread_rules` |
| A `modify` with a label that moves mail | `Api` 400, nothing sent | `writes_refuse_before_any_command` |
| A lost connection after the first `STORE` | `Transport` | `modify_refused_and_unconfirmed` |
| A wrong password | `Unauthorized` | `wrong_password` |
| A folder that is not in `from` (also one that the first sync listed after its start cursor) | `HistoryExpired`, then a clean full resync in pages of 2,000 | `changes_pages` |
| A page of changes and removals over the budget | `HistoryExpired` | `page_budget_counts_removals` |
| A LIST order change between two message pages | each folder once | `message_pages_follow_folder_names` |
| A flag that is not in the `PERMANENTFLAGS` of a target folder (no `\*` for a keyword) | `Api` 400 `notPermanent`, before any `STORE` | `modify_not_permanent` |
| A flag write of `UNREAD`, `STARRED` and a keyword, then a sync | the flags at the server change, zero differences | `modify_flags` |
| A label that is not a flag, or one label added and removed | `Api` 400, nothing sent | `modify_bad_labels` |
| A `NO` to a `STORE` | `Transport` | `modify_refused_and_unconfirmed` |
| A `\Sent` folder message | `SENT_COPY`, no `SENT` | `labels_of_special_folders_and_flags` |
| A server with no special-use attributes, or with `\Sent` only and a `Spam` folder with `\HasNoChildren` | the common names give each missing role; an attribute wins | `special_folders_by_name`, `name_fallback_for_each_role`, `an_attribute_wins_over_a_name` |
| `create_label("Todo")` | `Api` 400 `notSupported`, nothing written | `writes_refuse_before_any_command` |
| A folder name over 255 bytes | `Api` 400 `longFolderName` | `long_folder_name` |
| A thread header ID over 998 bytes | the own location ID as thread | `long_thread_header_is_ignored` |
| A keyword or folder named as a fixed label | a name with ` (keyword)` or ` (folder)` | `label_names_are_unique` |
| A new role of a folder | `HistoryExpired`, then a clean full resync | `folder_role_change` |
| Two folders with `\Junk` | `SPAM` on the mail of both, zero differences | `two_junk_folders_and_keyword_case` |
| One keyword in two spellings (`$Todo`, `$todo`) | one label `kw:$todo`, zero differences | `two_junk_folders_and_keyword_case` |
| An answer of `changes` above 1 MiB | `HistoryExpired` | `changes_too_large` |
| A read of a message in a deleted folder | `NotFound` | `folder_gone` |
| A `STORE` `OK` with no `FETCH` for a target UID | `Transport` | `modify_refused_and_unconfirmed` |
| `add-imap` of an address that another mailbox has | refused, nothing written | `add_imap_other_mailbox` |
| A Gmail sign-in of an address of an IMAP account | refused, nothing written | `gmail_sign_in_imap_address` |

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
- `UIDNEXT` comes with each `EXAMINE` (RFC 3501 requires it). `pending`:
  on a server with `CONDSTORE` and no `QRESYNC`, `profile` refuses a
  folder with no `UIDNEXT` (`Api` 400 `noUidnext`), because the removal
  rule of section 5 needs a value that only grows. The highest UID + 1 is
  no such value: a message that came and went after the cursor leaves it
  as it was. With `QRESYNC`, the `VANISHED` list covers removals, and a
  missing `UIDNEXT` does not matter.

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
4. Two folders with `\Junk` both give `SPAM`, and two spellings of one
   keyword give one label, with zero differences in `verify`.
5. On a server with no special-use attributes, a folder named `Sent
   Items` gives `SENT_COPY`, `Drafts` gives `UNSENT` and `Spam` gives
   `SPAM`.
6. A message of an IMAP account with `Authentication-Results:
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
4. The engine, in two PRs: first the received-mail helper and the trust
   rule (no effect on a Gmail account), then the migration, the command
   and the provider choice.
5. A later card: drafts and send over SMTP.
