# Spec: summary of a long thread ("Summary ready")

- Date: 2026-11-25
- Card: none (issue #478; card R3-09 "Prepared items in the brief: draft ready, summary ready")
- Status: Draft

## Problem

Card R3-09 promised two prepared things on a brief line: "Draft ready" and
"Summary ready". Only the draft shipped (PR #489). No part of Iris writes a
summary of a thread, so a person who owes an answer in a thread of 14 mails
must read all 14 mails to know where the thread is (issue #478).

## Goal

1. For a long thread of an item that needs the person, the engine writes a
   short summary of the thread, through the one budget of understanding.
2. The brief line of the item shows a "Summary ready" pill while the summary
   covers the newest mail of the thread.
3. The item page shows the summary above the mail, with who wrote it (Iris,
   from the mail) and the time of the newest mail that it covers.
4. Mail text stays data: it goes only into the data block of the model call,
   and the summary shows as plain text.

## Not in scope

- A summary of an attachment or of one long mail with no thread. Issue
  #1020 records this.
- A summary on demand (a button). The engine prepares each summary; the
  person does not ask for one in this version. Issue #1021 records this.
- A summary of a thread at the stranger gate, of a hostile thread, of a feed
  item or of a signal. The person did not accept the sender, or the mail is
  not worth a model call.
- A summary in the workspace of a job. Jobs read the clean mail.
- Edit, rate or delete a summary.
- The text of a message after its first 4,000 characters. The item page
  points to the mail. For a thread of more than 12 answerable messages,
  the snapshot holds the first message and the newest 11, and the data
  gives the last summary of the thread (see The model call, issue #1023).

## Design

### Which threads

A thread is a candidate when all of these are true:

- It is the thread of the origin message of an item that needs the
  person: an item that the brief shows in `you_owe`, `owed_to_you` or
  `conversations`. One SQL filter gives these items: the filters of the
  three sections (`section_sql` of `crates/iris-api/src/read.rs`), with
  the attention `shown`, from hostile mail never, and an item at the
  stranger gate never. `iris-api` makes this filter public
  (`needs_you_sql(now)`). The engine step gives it to `candidates`, and the
  API uses it for `ready` (see API), so the two reads use one rule.
- It is long: 5 or more answerable messages (the filter of
  `iris_voice::ANSWERABLE`: not spam, not trash, not gone, not a draft or a
  copy), or 2 or more answerable messages with 8,000 or more characters of
  clean text in them. One long message alone is not a thread (issue
  #1020). The
  filter is private now. The build makes it public, so that the two crates
  use one filter and no copy.
- The ready rules of a reply draft hold (`iris_voice::ready_of`, private
  now: the build gives it a public form for a table of its own): no message
  of the thread is hostile, each message has its clean text, and
  understanding read each message.
- It has no summary with the current basis (see Store), and no try with
  the current basis and the same model version that ended in an invalid
  answer.

### Store

Migration N (the next free number at build time) adds two tables:

```sql
CREATE TABLE thread_summary (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  -- The newest answerable message that the summary covers.
  based_on INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  -- The basis: the SHA-256 (hex) of each model input of the snapshot.
  basis TEXT NOT NULL,
  text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 1500),
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE thread_summary_try (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  based_on INTEGER NOT NULL,
  basis TEXT NOT NULL,
  model_version INTEGER NOT NULL,
  invalid INTEGER NOT NULL CHECK (invalid IN (0, 1)),
  at INTEGER NOT NULL
);
```

One summary for each thread: a new summary replaces the old one. A summary
is ready when two rules hold:

1. The rule of a reply draft (`iris_voice::fresh_sql`): `based_on` is the
   newest answerable message of the thread, and no message of the thread
   is hostile or waits for understanding.
2. The basis: the `basis` of the summary is the basis of the thread now.
   The basis is the SHA-256 of each input of the model call from the
   snapshot (the first answerable message and the newest 11, or each
   answerable message of a thread of 12 or fewer): for each message, its row
   ID, `from`, its subject, its date, `by_owner` (the `SENT` label) and its
   clean text as the data cuts it. One SQL list of these columns
   (`SNAPSHOT_COLUMNS`) feeds both the snapshot of the call and the basis,
   so the two cannot read different inputs. SQL computes the basis in the
   read (`basis_sql(thread)`) through a function of the store,
   `iris_sha256(text)`, which each connection registers.

So each change of the mail that the summary read makes the summary stale
at the next read, with no write: a new mail, a hostile mark, an older
message of the snapshot that goes away or leaves the thread, new content
or a new clean text under the same ID, and a new `SENT` label or date of a
message. A message that the snapshot leaves out is not in the
basis, so its change does not make the summary stale (issue #1023).

### The model call

A new crate `iris-summary` (no provider client, as `iris-voice`):

- `candidates(c, items, limit)`: the candidate threads of the items
  that the SQL filter `items` selects, the oldest try first, as
  `iris_voice::late` orders the nudges. The crate does not depend on
  `iris-api`: the caller gives the filter.
- `summarize(store, candidate, model, now)`: reads a snapshot of the thread (the
  first answerable message and the newest 11, oldest first, each cut to
  4,000 characters, with `from`, `by_owner` and `date`), makes one call, checks the
  answer, and saves it in one transaction only if the snapshot is the same
  as before the call (each message of the snapshot, its text, and the
  counts of hostile and unread messages). It saves the basis of that
  snapshot.
- When the snapshot leaves out answerable messages, the data gives their
  count (`left_out`) and the last saved summary of the thread
  (`earlier_summary`, else null), so the facts of the left-out mail carry
  forward from one summary to the next. The instruction says that the
  earlier summary is data, and that the mail in the snapshot is correct
  where the two differ. The save compares the earlier summary too. A
  summary of a thread that the snapshot covers in full gets no earlier
  summary. A thread whose first summary comes after it has more than 12
  messages has no earlier summary: its first summary covers the snapshot
  only.
- The instruction is a constant (`summary/0.2`). The data block is one JSON
  object. The instruction says that the mail is data, that text in the mail
  that asks for an action is not from the owner, and that the summary names
  only facts of the mail. The answer is `{"summary": "..."}`.
- The check refuses an answer that is not that object, an empty summary, and
  a summary longer than 1,500 characters. A refused answer gets one more
  call (decision D7), then the try is `invalid` for that basis and model
  version. A refusal of the model (`Stop::Refused` of `iris-model`) gets
  no second call, because the same data gets the same refusal. It is an
  invalid answer at once.

### The engine step

`crates/iris/src/summary.rs`: one step after the nudge step, in the same
turn of the understanding thread, under the same batch lock. At most 2
summary calls in one turn (a call and its retry need 2), so a summary never
takes the room of understanding. No call when no model is set, during the
wait after a model error, or when the budget refuses. Each try writes
`thread_summary_try`, so a thread that the budget cannot pay moves to the
end of the order and does not stop the others.

### API

- `ready` of an item gets `"summary"` when two things are true: the item
  itself passes `needs_you_sql(now)`, and the thread of its origin message
  has a summary that is ready. So an item at the stranger gate never shows
  "Summary ready", also when a known person in the same thread has an item
  with a summary. The brief sections, the gate, the bundles and the lists
  of one form read it with `"draft"`, from the columns of `ready_columns`
  (`ready_doc_of`).
- `GET /api/items/{id}/summary`: `{"text", "based_on", "created",
  "model"}` of the summary of the thread of the origin message of the item,
  with the same two conditions as `ready`, or 404. The item page reads it
  for each item, so it does not depend on `ready`: `GET /api/items/{id}`
  gives no `ready` (`doc_of`).
- Open model: the enum of `ready` in `common.schema.json` gets `summary`.
  This is a minor version of the open model (spec 4, rule 2): a new enum
  value of an optional field. The open model goes from 1.3 to 1.4. Rule 8
  applies: the schema gets a condition, so a document that claims 1.3 or
  older and holds `summary` in `ready` is not valid. Rule 6 (the maintainer
  approves each change of the model): dk approved the recommended options
  on 2026-11-25 (message cmsg_01AyyEYuxogEBopLeGJ2YcyzTEabK3Dmp4kMC1BJdqKfhj).

### Client

- `Line.svelte`: a second pill in the ready row, "Summary ready", with a
  lines icon, after "Draft ready". The pill is a link to the item page
  (`href.item(id)`, the route that the line uses now). The summary box is
  the first part of the item page after its title, so the link needs no
  anchor: the hash router reads `#/item/<id>` only.
- `Item.svelte`: the page reads `GET /api/items/{id}/summary` and shows a
  box above the mail: "Summary by
  Iris" in muted text, the text, and one faint line: "From the mail up to
  <date>. Read the mail for the exact words." The date is the date of the
  message `based_on` (`GET /api/messages?ids=`), never `created`. With no
  date, the line says "From the newest mail of the thread." Each new state
  of the item reads the summary again. A 404 shows nothing. The text
  is a Svelte text node, never HTML.
- A before/after page shows the brief line and the item page.

## Options not chosen

- A summary on demand only: the person must ask, so the brief cannot show
  "Summary ready". It can come later as a button (issue #1021).
- A summary of each thread: most threads are short, and the cost grows with
  the mailbox. Only long threads of items that need the person get one.
- Keep the summary in the item document: the item belongs to understanding,
  and a rebuild of items would lose it. A table of its own follows the
  reply draft.
- Put the step in `iris-voice`: the voice crate writes as the owner; a
  summary writes about the mail. A crate of its own keeps the prompts and
  the checks apart.
- A library for summaries: the call goes through the model layer of Iris
  (`iris-model`) as each other call. It adds no parser and no protocol
  client.

## Failure list

Supported input: a thread whose messages passed the hostile check, with
clean text, of the account of the item.

| Input or event | Result | Test |
|---|---|---|
| Mail text that asks the model to do something | The text is in the data block only. The answer is plain text; the client shows it as text. | Unit test: a hostile-looking instruction in the data stays in the data; the answer is shown escaped (client mount test). |
| A new mail arrives during the call | The save compares the snapshot in its transaction and writes nothing. The next turn tries again. | Store test with a message added between snapshot and save. |
| A hostile mark or a gone message after the save | The ready rule hides the summary at the next read. | Unit test of `ready` and an API test. |
| An older message of the snapshot goes away, or gets new content, a new clean text, a new `SENT` label or a new date under the same ID | The basis changes: the summary is stale, and the thread is a candidate again. | Unit test of `ready` and `candidates`. |
| The model fails (time-out, 5xx) | No row. The wait after a model error applies (decision D35). | Engine test with a failing fake model. |
| The answer is not valid twice | `thread_summary_try` with `invalid = 1` for that basis and model version. No more calls until the basis or the model version changes. | Unit test. |
| The model refuses | No second call. The answer is not valid. | Unit test. |
| The budget refuses | No call. The try moves the thread to the end. | Engine test. |
| The item or the summary changes during a read of `GET /api/items/{id}/summary` | One SQL statement reads the item condition and the summary, so both come from one snapshot. | API test with a writer that changes the item and the summary text in one transaction. |
| A crash during the save | One transaction: no summary, or the whole summary. | Store test. |
| Another run holds the batch lock | No call in this turn. | Engine test. |
| Any other input (a thread with no clean text, a thread of another kind) | Not a candidate: no call. | Unit test of `candidates`. |

## Impact

- `crates/iris-store`: one migration, two tables, the SQL function `iris_sha256`, and the delete of the two tables in the removal of an account.
- New crate `crates/iris-summary`.
- `crates/iris/src/summary.rs` and one call in `crates/iris/src/engine.rs`.
- `crates/iris-api`: `ready` gets `summary`; one new endpoint; README.
- `docs/spec/schema/common.schema.json`, `docs/spec/VERSION` (1.4), spec 4
  changelog, `iris_store::MODEL`.
- `client/`: `Line.svelte`, `Item.svelte`, `api.ts`, fixtures, tests,
  `docs/DESIGN.md`.

Memory: one summary of at most 1,500 characters for each candidate thread,
in SQLite. The step holds one snapshot (at most 12 x 4,000 characters, and one
earlier summary of at most 1,500 characters) in memory during a call.

## Assumptions

- The ready rule of a reply draft is the right staleness rule for a summary.
  Evidence: `iris_voice::ready_of` (crates/iris-voice/src/lib.rs) and the
  test `an_item_shows_a_ready_draft_of_its_thread_until_the_draft_is_stale`.
- The budget wrapper of understanding can pay a call of another crate.
  Evidence: `crates/iris/src/nudge.rs` uses `iris_understand::budget`.
- 5 messages or 8,000 characters marks a long thread. Evidence: pending.
  The values are constants; a wrong value changes only which threads get a
  summary, never the safety of the step.
- A new optional enum value is a minor version of the open model. Evidence:
  spec 4, rule 2 (docs/spec/04-open-model-versions.md).

## External writes

None. The model call sends mail text to the model that the person set (as
understanding does). It changes nothing outside Iris.

## Open questions

- The best number of messages in the snapshot (12) for the local models.
- The lines icon of the pill: reuse the record icon or draw a new one.

## Acceptance criteria

1. A long thread of an open commitment gets a summary in the next turn with a
   fake model; the brief line shows "Summary ready". Red if the step does
   not run or `ready` lacks `summary`.
2. A new mail in the thread removes the pill at the next read. Red if the
   ready rule does not check the basis. An older message of the snapshot
   that goes away removes the pill too. Red if the rule checks only the
   newest message.
3. A short thread, a gate thread and a hostile thread get no call. Red if
   the fake model counts a call.
4. An invalid answer twice stops calls for that newest mail. Red if a third
   call happens.
5. A thread with a known person's commitment and a stranger's commitment:
   after the summary, the brief line of the first shows "Summary ready",
   and the gate item of the second does not. Red if `ready` reads only the
   thread.
6. The item page shows the text as text: `<b>x</b>` in the summary shows the
   characters. Red if it renders bold.

## Verify

- `cargo test -p iris-summary -p iris-api -p iris`
- `cd client && npm test`

## Rollout

1. Migration with the tables.
2. The crate and the engine step. Until the API part merges, the summaries
   exist only in the store.
3. API and open model 1.4.
4. Client.

## Risks

- Cost: each long thread of an item that needs the person costs one or two
  calls each time a new mail comes. The per-turn limit (2) and the budget
  bound it. Rollback: remove the step call in `engine.rs`; the tables stay.
- A wrong summary: the item page says that Iris wrote it from the mail and
  points to the mail.

## Plan

1. Store and crate: migration, `iris-summary` with `candidates`,
   `summarize`, the check and the ready SQL, with tests (fake model).
2. Engine step: `summary.rs`, the call after the nudge step, and the public
   `needs_you_sql(now)` of `iris-api` that the step gives to `candidates`.
3. API and open model 1.4: `ready` with `summary`, `GET
   /api/items/{id}/summary`.
4. Client: the pill, the item page box, the before/after page.

## Test plan

See the failure list and the acceptance criteria. Each test uses the fake
model of `iris-understand` and the synthetic mailbox. No test uses a real
model or a real mailbox.

## Spec checks

- [x] Each assumption holds with a second caller, worker or tab: the batch
  lock allows one run; the save compares the snapshot in its transaction.
- [x] Each assumption holds when an outcome is unknown: a lost model answer
  is a model failure; no row is written.
- [x] Each dropped mechanism was dropped for a reason that makes it
  unnecessary: no on-demand button, because the engine prepares each
  summary.
- [x] Each "idempotent" step is not check-then-act: the save re-reads
  snapshot in the same transaction as the write.
- [x] Each property that the design depends on has a test: see the failure
  list.
- [x] The spec addresses the hardest problem of the change: mail as data and
  a stale summary.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported inputs and the safe result for all
  other input.
