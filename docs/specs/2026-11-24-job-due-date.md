# Spec: the due date of a job moves it to needs you

- Date: 2026-11-24
- Card: P9-D24
- Status: Built

## Problem

A job has a due date (`due_at`), but nothing happens on that date (issue
#248). A job past its due date looks the same as a job before it. The
person sees the late job only if they open the job page and read the date.

Decision D24 (dk, 2026-11-24, `docs/decisions.md`): on the due date, the job
moves to "needs you", with a notice. Ending the job is too strong.

## Goal

- On its due date, the engine moves each job in `working` or `waiting` to
  `needs_you`, once for each due date.
- The question of the move tells the person what happened and what they can
  do.
- The engine shows a system notice for each move.
- The person answers the question (the agent goes on with the answer), or
  ends the job, as for each other question.

## Not in scope

- A change of the due date after the start. Card P9-D25 (decision D25)
  adds the edit. This spec says how an edit and this move work together.
- A notice for the questions of an agent (`iris ask`). They have none
  today. This card does not change that.
- A row in the notice list of the web client or of the Mac app for the
  move. See "Known limits".

## Design

### The move

The move table of `docs/specs/2026-11-16-job-store.md` gets one row:

| From | To | Who | Name | Values |
|---|---|---|---|---|
| `working`, `waiting` | `needs_you` | engine | due | `question`, `why` = `due`, `at` not before `due_at` |

A new migration (the next free number at the build) drops the trigger `job_event_move` and makes it again with this
row. Each other row of the table stays the same. The trigger refuses a `due`
move of a job with no due date, a move with `at` before `due_at`, and a move
by the engine to `needs_you` with another `why`.

The move to `needs_you` ends the open session of the job (trigger
`job_session_job_state` of migration 21), the same as an `ask`. The
supervisor then stops the processes of the session.

The question is fixed engine text, not mail text:

> The due date passed. Answer to go on, or end the job.

The answer moves the job to `working`, as each answer does. If the job had
a session, the answer starts a new session with the agent of the last
session (`iris_job::woken`). A job with no session (the person never ran an
agent on it) starts no session: the job page shows the command to run an
agent, as for each job in `working` with no session. So the question does
not promise that an agent goes on.

### When the move is due

A job is due at `now` when all of these are true:

1. The job is in `working` or `waiting`.
2. `due_at` is not NULL and `due_at <= now`.
3. No event of the job at or after `due_at` is a move to `needs_you` (by
   the agent or the engine), or a move by the person. The first event of
   the job (the start) does not count: a job that the person starts after
   its due date moves on the next turn.

Rule 3 makes the move happen once for each due date. After the move, or
after the person acted on the job after the due date (an answer, a wake),
the person knows that the date passed. A later due date (card P9-D25) has no
event after it yet, so it moves the job again on that date.

A job in `needs_you` on its due date does not move: it already waits for the
person. If the person answers after the due date, rule 3 sees the answer, and
the job does not move again.

### The engine step

`iris_job::move_due(store, now_ms) -> Result<Vec<Due>>` reads the due jobs
(`due_jobs`, index `job_state`), then makes each move in its own write
transaction that checks rules 1 to 3 again (`move_due_one`). A move between
the read and the write that makes a rule false wins, and the step makes no
move: a finish, an ask, or a move of the person. A wait of the agent after
the due date keeps the rules true, so the job still moves: the due date
passed, and the person decides. `Due` holds the job ID and its goal.

The attention loop of the engine (`crates/iris/src/engine.rs`, each 10
seconds) runs the step, then gives one system notice for each moved job to
the notice thread (`crates/iris/src/notify.rs`):

- Title: `Iris: Job due`
- Text: `j-<n>: <goal>`, cut to 200 characters on a character boundary.

The goal is the words of the person, never mail text. The notice command
gets it as an argument, as for the notices by rule.

The notice thread gets a plain title and text in place of
`iris_policy::notice::Notice`, so the notices by rule and the job notices use
one queue.

## Options not chosen

- End the job on its due date (option B of D24): dk chose A.
- A row in the `notice` table for the move: the table has one row for each
  message (`message_id NOT NULL UNIQUE`). A job notice needs a new kind of
  row, a change of `/api/notices`, the notice list of the web client and the
  Mac app. The job page and the "Needs you" group of the jobs page show the
  state. See "Known limits".
- A fixed time of the day for the move: the due date of the start page is a
  time already (the end of the day of the person).

## Failure list

| Input or event | Result | Test |
|---|---|---|
| A job with no due date | No move | Unit test |
| A job before its due date | No move | Unit test |
| A job in `needs_you` or `done` on its due date | No move | Unit test |
| A job that moved to `needs_you` after its due date (engine or agent) | No second move | Unit test |
| The person answered or woke the job after the due date | No move | Unit test |
| The move of one due job fails (for example a goal that is not UTF-8 from raw SQL) | That job does not move and stays in its state. The other due jobs still move and get their notices. Each turn writes one log line that names the job | Unit test (`a_job_whose_goal_is_not_utf8_does_not_stop_the_others`) |
| A job that starts after its due date | One move on the next turn | Unit test (`a_job_that_starts_after_its_due_date_moves_once`) |
| The engine turn with a due job | The move and one notice; the next turn gives no second notice | Engine test (`the_attention_turn_moves_a_due_job_once`) |
| A move between the read and the write that makes rule 1, 2 or 3 false | The write sees it and makes no move | Unit test (`a_move_after_the_read_wins`) |
| A wait of the agent between the read and the write | The job still moves | Rules 1 to 3 hold |
| A `due` move by the agent or the person, with no due date, before `due_at`, or with another `why` | The trigger refuses it | Unit test |
| A crash after the commit of the move and before the notice | The job stays in `needs_you`. No notice is shown. The jobs page shows it | Known limit |
| The queue of system notices is full | The notice is dropped. The jobs page shows the job | Known limit |
| No notice command on the system | One log line. The jobs page shows the job | Existing behavior of `notify::show` |

## Impact

- `crates/iris-store/src/schema_<n>.sql` (the next free number): the trigger `job_event_move` again,
  with the `due` row.
- `crates/iris-job`: `move_due` and `Due`.
- `crates/iris/src/engine.rs`, `crates/iris/src/notify.rs`: the step in the
  attention loop, and a plain queue of system notices.
- `docs/specs/2026-11-16-job-store.md`: the move table and the paragraph on
  the due date.
- No change to the job document or its schema: an event by the engine with
  the state `needs_you` and a question is valid now.

## Assumptions

- The job document schema allows an event by `engine` with the state
  `needs_you`. Evidence: `docs/spec/schema/job.schema.json`, `by` enum and
  the `needs_you` rule.
- A move out of `working` ends the open session. Evidence:
  `crates/iris-store/src/schema_21.sql`, trigger `job_session_job_state`.
- An answer of the person starts a new session. Evidence:
  `iris_job::woken` (an event with an answer).

## External writes

None. The move writes only the store. The notice is a local command.

## Open questions

None.

## Acceptance criteria

1. A job in `working` or `waiting` moves to `needs_you` by the engine at its
   due date, with the fixed question and `why` = `due`. Red if the step
   skips the job.
2. The move happens once: the next steps make no move, also after the
   answer of the person. Red if rule 3 is missing.
3. The engine gives one system notice for each move. Red if the step gives
   no notice.
4. After the answer, a job with a session gets a new session, and a job
   with no session stays in `working` with no session. Red if the answer
   of a due job with no session fails or starts a session with no agent.
5. The trigger refuses a `due` move that the move table does not allow. Red
   if the trigger is not changed.

## Verify

```bash
cargo test -p iris-job --test due
cargo test -p iris --lib notify
cargo test -p iris-store
```

## Known limits

- The move has no row in the notice list of the web client and of the Mac
  app. The system notice and the jobs page show it (issue #844).
- A crash between the commit of the move and the system notice loses the
  notice. A full queue of system notices (100) drops it too. The job stays in `needs_you` (issue #845).

## Rollout

The new migration runs at the next start of the engine.

## Risks

A wrong rule 3 can move a job again and again. The unit tests cover each
case of rule 3.

Rollback: an old engine refuses a store with a newer schema version
(`Error::UnknownSchema`), so a revert of the code alone does not open the
store. Fix forward with a new PR, or stop the engine and restore the backup
that the migration made (`iris.db.v<n>.<time>-<random>.bak` in the data
folder, specification 1, section 5.5). A restore loses the changes after the
migration.

## Plan

1. P9-D24: the migration, `move_due`, the engine step and the notice.
