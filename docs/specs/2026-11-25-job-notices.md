# Spec: a notice row when a job needs you

- Date: 2026-11-25
- Card: none (open-issues sweep of 2026-11-25; issues #844 and #845)
- Status: Agreed

## Problem

When a job moves to `needs_you`, the person must act, but the notice list
does not show it:

- On its due date, the engine moves a job to `needs_you` and shows a system
  notice (decision D24, `docs/specs/2026-11-24-job-due-date.md`). The move
  has no row in the notice list of the web client or of the Mac app (#844).
- A crash after the commit of the move, or a full queue of system notices,
  loses that system notice (#845). Nothing else tells the person.
- A question of an agent (`iris ask`) also moves the job to `needs_you`. It
  has no notice of any kind.

The table `notice` has one row for each message (`message_id NOT NULL
UNIQUE`), so a job cannot have a row today.

## Goal

- Each move of a job to `needs_you` (a question of an agent, or the due
  date) adds one notice row in the same write transaction as the move. A
  crash cannot keep the move and lose the row.
- The notice list of the web client and of the Mac app shows the row while
  the job waits on that question. The row closes by itself when the job
  leaves that question (an answer, an end), or when the person marks it
  done.
- The engine or the Mac app shows one system notice for each new `Job due`
  row, with the rules of the notices by rule (one owner for each notice).
- A `Job question` row gives no system notice. Iris interrupts only by a
  rule that the person wrote, or on the due date of a job (`PRODUCT.md`,
  "Do not interrupt"; `docs/manifesto.md`, principle 3). An agent can ask
  at any time, so its question waits quietly in the notice list.
- The words of a job notice are the words of the person (the goal), never
  the text of an agent or of a mail.

## Not in scope

- A notice for a move to `waiting` or `done`. They need no act.
- A system notice for a question of an agent. It needs a change of the
  interrupt rule of `PRODUCT.md`, and that is a product decision.
- The text of the question in a notice. The job page shows it.
- A new screen. The notice list gets the job rows in its present design.

## Design

### The store

Migration `N` (the next free number at merge; 59 on 2026-11-25) makes the
table `notice` again, with the same row IDs and the same `sqlite_sequence`
value, so the engine never uses an ID again (specification 1, 5.7):

| Column | Rule notice | Job notice |
|---|---|---|
| `id` | as before | as before |
| `message_id` | the mail, `UNIQUE` | NULL |
| `job_id` | NULL | the job |
| `job_event_id` | NULL | the event of the move, `UNIQUE` |
| `rule`, `policy_version` | as before | NULL |
| `rule_name` | the name of the rule | `Job due` (a move by the engine) or `Job question` (a move by the agent) |
| `title` | the sender | `j-<n>` |
| `text` | the subject | the goal, at most 2,000 characters |
| `created_at`, `done_at` | as before | as before |

A `CHECK` holds that a row has a mail or a job event, never both and never
none, and that a job row has no rule. No foreign key: a notice keeps its
words, as before.

A trigger `AFTER INSERT ON job_event WHEN NEW.state = 'needs_you'` inserts
the job row. Each path that moves a job to `needs_you` inserts a
`job_event` row (`add_event`), so the trigger covers the paths by
construction: `iris ask`, `move_due_one`, and each later path.

The copy of the old rows keeps `id`, and an insert into `sqlite_sequence`
keeps the old `seq` when it is larger than the largest `id`.

### Open notices

The condition "open" (`iris_policy::notice`) gets one more term: a job row
is open only while its event is the newest event of its job. An answer, a
wait or an end adds a newer event, so the row closes with no write. The
index `job_event_job (job_id, id)` serves the term. The terms for hostile
mail do not change: a job row has no mail.

### The system notice

A row "interrupts" when it is a rule row, or a job row of a move by the
engine (`Job due`). A `Job question` row never interrupts. The term reads
`job_event.by` of the event of the row, not the name, because a rule name
is text of the person and can be any words.

The engine keeps the largest job notice ID that it saw, in memory. It starts
at the largest notice ID at the start of the attention thread, so a restart
shows no old notice. Each turn reads the open `Job due` rows above it,
oldest first, and gives them to `notify::for_system` with the rule rows. So
the Mac app or the engine shows each one, never both, as for the notices by
rule.

- Title: `Iris: Job due`.
- Text: `j-<n>: <goal>`, one line, cut to 200 characters.

`Shown::job_due` and the notice of `DueTurn::moved` go: the row is the one
source. `DueTurn` keeps `moved` for the log and the tests.

A crash or a full queue can still lose the system notice. The row stays in
the notice list and the Mac app reads it at its next call, so the person
still sees the job (#845).

### The API

`/api/notices` and `app.notices` give each row with:

- `rule`: the rule number, or `null` for a job row.
- `job`: the `j-` ID of the job, or `null` for a rule row (new field).
- `rule_name`, `title`, `text`, `created`, `message` as before (`message`
  is `null` for a job row).

`app.notices` gives in `notices` only the rows that interrupt: the Mac app
shows a system notice for each row of that list. `open` counts each open
row, the `Job question` rows too, so the count of the menu bar includes
them. `last` stays the largest row ID of the answer, or `after`, so the
cursor of the app passes a `Job question` row with no system notice only
when a later row that interrupts comes.

A Mac app from before this change reads `rule_name`, `title` and `text`,
and opens `#/brief`, so it shows a `Job due` row with no change. The new
Mac app opens `#/jobs/<j-id>`.

`POST /api/notices/<n-id>/done` marks a job row done as a rule row.

### The web client

The notice list shows a job row as a rule row: the name (`Job due` or
`Job question`), then a link `j-<n>: <goal>` to the job page. The count line
says "notices", not "notices by your rules", because a job row comes from a
job. No new token and no new part.

## Options not chosen

- A second table `job_notice`: the Mac app pages the notices by one row ID
  (`after`), so two tables would need one shared sequence.
- The insert in Rust in `add_event`: a trigger covers each path with no
  code in each caller.
- The text of the agent question in the system notice: an agent reads mail
  text, so its question can carry the words of a mail into a system banner.
  The goal is the words of the person.
- Close the row with a write at each move out of `needs_you`: the newest
  event term needs no write and cannot miss a path.

## Failure list

| Input or event | Result | Test |
|---|---|---|
| A move to `needs_you` by the agent or by the due date | One open job row in the same transaction | store test |
| The move fails or rolls back | No row | store test |
| An answer, a wait or an end after the move | The row is no longer open | policy test |
| A second question later | A second row; the first stays closed | policy test |
| A crash after the commit | The row is in the list; the engine shows no old row after the restart | engine test |
| A full queue of system notices | The row stays in the list and in the Mac app | known behavior of the queue |
| The Mac app is live | The app shows the `Job due` row; the engine does not | engine test with `for_system` |
| A question of an agent | One open `Job question` row in the list and in the count of `app.notices`; no system notice from the engine or the Mac app | engine test, socket test |
| A rule whose name is `Job due` or `Job question` | It interrupts as each rule row; the name changes nothing | policy test |
| Old rule notices at the migration | Same IDs, same columns, same open state | migration test |
| A `notice` table whose largest row was deleted | `sqlite_sequence` keeps the old value | migration test |
| A row with both a mail and a job, or neither | The `CHECK` refuses it | migration test |

## Impact

- `crates/iris-store`: the migration.
- `crates/iris-policy/src/notice.rs`: the open term, the interrupt term, the read of new `Job due` rows, the optional rule.
- `crates/iris-job`: no change of code (the trigger), tests.
- `crates/iris/src/engine.rs`, `notify.rs`: the system notice from the row.
- `crates/iris-api/src/notice.rs`, `crates/iris/src/socket.rs`: the `job` field, the optional `rule`.
- `client/src/notices.ts`, `client/src/views/Notices.svelte`: the job row.
- `apps/mac/src/engine.rs`: the route of a job row.
- `docs/spec/01-mirror-store-forms.md` (the `notice` table), `docs/specs/2026-11-24-job-due-date.md` and `docs/specs/2026-11-16-notices.md`.

Three PRs: the engine (store, policy, engine, API), the web client, the Mac
app.

## Assumptions

- Each move to `needs_you` inserts a `job_event` row with `state =
  'needs_you'`. Evidence: `add_event` in `crates/iris-job/src/lib.rs` is the
  only insert into `job_event`, and the `CHECK` of `job_event` ties
  `needs_you` to a question.
- No code deletes `job_event` rows. Evidence: `grep -rn "DELETE FROM job_event" crates/` finds only a test that checks the refusal.
- The Mac app reads only `rule_name`, `title`, `text` and `message.id` of a
  notice. Evidence: `notice()` in `apps/mac/src/engine.rs`.

## Open questions

None.
