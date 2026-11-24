# Spec: start a job from a typed goal, and edit a job after the start

- Date: 2026-11-24
- Card: P9-D25
- Status: Built (store and API); the web client follows

## Problem

A job starts only from a commitment or a conversation ("Handle this",
`docs/specs/2026-11-17-start-job.md`). A goal with no mail, for example
"Book a dentist appointment", has no path to an agent. After the start, the
goal, the due date and the scope do not change: the job store refuses each
change of `goal` and `due_at`, and the web client cannot add a thread to the
scope or take one out (issue #384).

Decision D25 (dk, 2026-11-24, `docs/decisions.md`): do both. Start a job with
no mail, and edit the goal, the due date and the scope (add a thread from a
search). "Book a dentist" is the most common agent goal, and a wrong scope
must be fixable.

## Goal

1. The person starts a job from a typed goal and an optional due date, with
   no mail in its scope.
2. The person changes the goal and the due date of an open job. The store
   keeps each change.
3. The person adds a thread to the scope of an open job from a search, and
   sees each message that the add grants before the add.
4. The person takes a thread or an item out of the scope of an open job.

## Not in scope

- A change by the agent. Only the person edits a job (the job store rule:
  an agent cannot widen its own scope).
- A list of the edits on the job page. The store keeps them. See "Known
  limits".
- A new goal for a session that runs. The agent reads the goal at the start
  of its session (`JOB.md`). See "Known limits".
- An add of one message or of one item from a search. The add is one
  thread.

## Design

### Store: the edit of a job

Migration 50 adds the table `job_edit`:

| Column | Type | Meaning |
|---|---|---|
| `id` | INTEGER PRIMARY KEY AUTOINCREMENT | The order of the edits |
| `job_id` | INTEGER NOT NULL REFERENCES job(id) | The job |
| `at` | INTEGER NOT NULL | Milliseconds since 1970 |
| `goal_before` | TEXT NOT NULL | The goal before the edit |
| `due_before` | INTEGER | The due date before the edit, or NULL |
| `goal` | TEXT NOT NULL | The goal after the edit (the checks of `job.goal`) |
| `due_at` | INTEGER | The due date after the edit, or NULL (the checks of `job.due_at`) |

An edit holds the full values before and after the edit. So the first edit
keeps the goal and the due date of the start, also for a job that existed
before the migration, and the newest edit is the truth. The triggers:

- `job_edit_insert` (BEFORE INSERT) refuses an edit of a job that does not
  exist or is `done`, an edit with `at` before the newest edit of the job,
  an edit whose before-values are not the values of the job row now, and
  an edit that changes nothing.
- `job_edit_apply` (AFTER INSERT) copies `goal` and `due_at` to the job
  row.
- The trigger `job_row` of migration 14 is made again. It allows a change of
  `goal` and `due_at` only when the new values are the values of the newest
  edit of the job. Each other rule of `job_row` stays.
- `job_edit` rows are never changed or deleted.

An edit is not a `job_event`: it does not move the job, does not change
`updated_at`, and does not end a session. So an answer stays bound to its
question (`Asked` uses `updated`), and a goal edit does not stop an agent.

The due date and decision D24 (`docs/specs/2026-11-24-job-due-date.md`): the
due move happens once for each due date. A new due date that is later than
the last move has no event after it yet, so the job moves again on that
date. A new due date in the past moves an open job at the next step.

`iris_job::edit_in(c, job, goal, due_ms, now_ms)` writes one edit, by the
person only (no `By` argument: only the API of the person calls it).

### Store: the start with no mail

`iris_job::create_in` is not changed: a job with no scope row is valid now.
The workspace of such a job has `JOB.md` and no mail files.

A typed start has no item that makes it idempotent. So the client makes one
random request ID (a UUID) when it opens the form, and sends it with each
try of that form. The migration adds the table `job_request`
(`request TEXT PRIMARY KEY` with 1 to 64 characters, `job_id INTEGER NOT
NULL UNIQUE REFERENCES job(id)`). The start writes the job and the request
row in one transaction. A start with a request ID that has a row makes no
job and answers with the job of the row. A new form is a new request ID, so
a second job on purpose is a second form.

### API

| Route | Body | Answer |
|---|---|---|
| `POST /api/jobs` | `{"goal", "due"?, "request"}` (no `item`, no `messages`) | 201 and the job document; 409 with `job` for a request ID that has a job |
| `POST /api/jobs/{id}/edit` | `{"goal", "due"}` (`due` null for no due date) | 200 and the job document |
| `GET /api/jobs/{id}/scope/{thread}` | none | `{"messages": [...]}`: the preview of the add, each summary with `readable` |
| `POST /api/jobs/{id}/scope` | `{"thread", "messages"}` | 200 and the job page |
| `POST /api/jobs/{id}/scope/remove` | `{"ref"}` (`t-<n>` or the item ID) | 200 and the job page |

- `POST /api/jobs` with `item` keeps its behavior. With no `item`, a
  `messages` field is a 400, and a missing or empty `request` is a 400. The
  goal and the due date have the checks of the start with an item. A retry
  with the same request ID (after a lost answer, a time-out or a reload of
  the form with its draft) gets 409 and `job`, the same answer as a second
  start from an item, and the client opens that job.
- `edit`: the same checks of the goal and the due date as the start. A job
  in `done` is a 409.
- The preview of an add lists each message of the thread now, in the order
  of the start preview. The add takes the message IDs that the person saw,
  and in one write transaction compares them with the messages of the thread
  now. If they differ, 409 "The mail changed. Look at it again." and nothing
  changes. Else `add_scope_in(job, Ref::Thread(t), By::Person, now)`.
- `remove` calls `revoke_scope_in`. The revoke ends the open session of the
  job (existing behavior), so the agent cannot read the mail that the person
  took out. `revoke_scope_in` and the trigger `job_scope_update` refuse a
  revoke for a job in `done`: the scope of a done job is its record.
- An item ID names its form (`c-3`). A later understanding can delete the
  item row of an item ref. Migration 50 adds the table `job_scope_form`: the
  form of the item of each item ref, at the grant (a trigger on the insert
  of the ref). The scope list and `remove` read the form from the item row,
  else from this table, so the ref keeps its ID and the person can remove
  it.
- The job page (`GET /api/jobs/{id}`) gets a `scope` list: each active ref
  with its ID, its kind and a label (the subject of the first message of a
  thread, the title of an item), and the count of granted messages.

### Web client

- Jobs page: a "New job" button opens a form with the goal and the due
  date. "Start the job" opens the job page.
- Job page, open job: "Edit" next to the goal opens the goal and the due
  date in place, with "Save" and "Cancel".
- Job page, open job: a "Mail the agent reads" section lists the scope, each
  ref with "Remove". "Add mail" opens a search of the mail; a press on a
  result shows each message of its thread (the add preview, with "Hidden
  from the agent for now" on a message that it cannot read), then "Add this
  thread".
- The before/after page shows the jobs page and the job page. dk picks
  before the client PR merges.

## Options not chosen

- Edit as a `job_event` move to the same state: the move table has no move
  to the same state, and a move changes `updated_at`, which binds an answer
  to its question.
- Write `goal` and `due_at` on the job row with no history: the store keeps
  each change of a job (job store spec).
- End the session at each edit, so the agent reads the new goal at once: an
  edit of a typo must not stop the work.
- Add one message, not a thread: a thread is the unit of the scope of a
  start, and a reply wake reads a thread ref.

## Failure list

| Input or event | Result | Test |
|---|---|---|
| A goal that is empty, longer than 2000 characters or with NUL | 400 | API test |
| A due date that is not RFC 3339 from 1970 to 9999 | 400 | API test |
| A start with no item and with `messages` | 400 | API test |
| An edit, an add or a remove on a `done` job | 409, nothing changes | API test |
| An edit that changes nothing | 409, nothing changes | Store test |
| An edit or a goal change by SQL with no edit row | The trigger refuses it | Store test |
| An add of a thread that does not exist | 404 | API test |
| New mail in the thread after the preview | 409 "The mail changed", nothing changes | API test |
| An add of a thread in the scope already | The add adds the messages of the thread now to its list (existing `add_scope_in`) | API test |
| A remove of a ref that is not active in the scope | 404 | API test |
| A remove of an item ref whose item row a later understanding deleted | The scope list keeps the ID of the ref, and the remove works | API test |
| A revoke by SQL for a `done` job | The trigger refuses it | API test |
| A remove while a session runs | The revoke ends the session (existing) | Existing test |
| A typed start that the client sends again with the same request ID | 409 with the job of the first start, no second job | API test |
| Two edits at the same time | One transaction each; the second edit is the newest | Store test |

## Impact

- `crates/iris-store`: migration 50 (`job_edit`, `job_request`,
  `job_scope_form`, the triggers `job_row` and `job_scope_update` again). The migration uses `IF NOT EXISTS`, because a
  test store that sets an older `user_version` runs it again.
- `crates/iris-job`: `edit_in`, `edit`.
- `crates/iris-api`: the routes above, the `scope` list of the job page.
- `client/`: the jobs page, the job page, a new add view.
- `docs/specs/2026-11-16-job-store.md`: the goal and the due date change
  only by an edit.
- `docs/specs/2026-11-17-start-job.md`: the start with no item.

## Assumptions

- A job with no scope row can run a session. Evidence: the workspace
  build writes `JOB.md` and one file for each ref
  (`crates/iris-workspace/src/lib.rs`), and `setup` of
  `crates/iris-workspace/tests/notes.rs` builds the workspace of a job with
  no scope.
- `revoke_scope_in` ends the open session. Evidence: the session launcher
  spec, "A session that a revoke or a stop ended".
- `GET /api/search` gives the thread of each message. Evidence: the
  summary has `thread`.

## External writes

None. Each change writes only the store.

## Open questions

None.

## Acceptance criteria

1. `POST /api/jobs` with a goal and no item makes a job with no scope. Red
   if the route still needs an item.
2. An edit changes the goal and the due date of the job document, keeps the
   values before and after in `job_edit`, and does not change `updated_at`. Red if the
   trigger refuses the edit, or if a direct `UPDATE` of `goal` passes.
3. An add grants the messages that the person saw, and refuses when the
   thread changed. Red if the add skips the comparison.
4. A remove revokes the ref. Red if the scope list still shows it.
5. In the web client, the person does each of 1 to 4. Red if a control is
   missing.

## Verify

```bash
cargo test -p iris-job --test edit
cargo test -p iris-api --test start
cd client && pnpm test
```

## Known limits

- The job page shows the goal and the due date now, not the list of the
  edits (issue #846).
- A session that runs keeps the goal of its start. The next session reads
  the new goal (issue #847).

## Rollout

The migration runs at the next start of the engine.

## Risks

A wrong `job_row` trigger can allow a change of the goal with no history.
The store tests cover the direct `UPDATE`.

Rollback: an old engine refuses a store with a newer schema version
(`Error::UnknownSchema`), so a revert of the code alone does not open the
store. Fix forward with a new PR, or stop the engine and restore the backup
that the migration made (`iris.db.v<n>.<time>-<random>.bak` in the data
folder, specification 1, section 5.5). A restore loses the changes after the
migration.

## Plan

1. P9-D25: store and API (one PR).
2. P9-D25: web client, after dk's pick on the before/after page (one PR).
