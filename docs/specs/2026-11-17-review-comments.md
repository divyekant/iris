# Spec: review comments to the agent

- Date: 2026-11-17
- Card: R4-08
- Status: Draft

## Problem

Spec 2, section 5.5 says: "A comment goes back to the author. In release 4,
the comment arrives as a file in the workspace. The author submits a new
revision." The verb `iris revise` exists (R4-03,
`docs/specs/2026-11-17-job-verbs.md`), but the person cannot write a
comment, the store has no place for a comment, and the workspace has no
comment file. So the agent of a job cannot learn why the person denied a
change, and the only way to change a proposal is a new job.

## Goal

1. The person writes a comment on a submitted changeset of a job in the web
   client. The store keeps each comment. Nothing changes a comment after
   the write.
2. The next build of the job workspace writes the comments of each
   changeset of the job into `comments/cs-<id>.md`, and `JOB.md` names the
   files and tells the agent to make a new revision.
3. A comment starts a new session of the job, and that session submits a
   new revision (the card). Stage 2 below adds the start, after R5-04
   merges.

## Not in scope

- A comment on a changeset of triage or of the person. These authors read
  no comment. An agent with no job (the MCP host) and a `local` author take
  a comment since decision D15 (issue #538): they read it in the changeset
  document (section 3).
- An edit or a delete of a comment. A comment is a record of what the
  person said.
- An answer of the agent to a comment. The new revision and its `why` are
  the answer.
- The edit of the text of a draft in the review (issue #208).
- The wait verb and its wake rules (R5-04).

## Design

### 1. The store

Migration `<n>` (the next free number at the build) adds this table:

| Column | Type | Meaning |
|---|---|---|
| `id` | `INTEGER PRIMARY KEY AUTOINCREMENT` | The comment ID. Never used again. |
| `changeset_id` | `INTEGER NOT NULL REFERENCES changeset(id)` | The changeset. |
| `revision` | `INTEGER NOT NULL CHECK (revision >= 1)` | The revision that the person saw. |
| `text` | `TEXT NOT NULL` | The comment. 1 to 4,000 characters, not only white space, no NUL. |
| `key` | `TEXT NOT NULL` | A key that the client makes for each new comment (16 to 64 characters of `[A-Za-z0-9_-]`). `UNIQUE (changeset_id, key)`. |
| `at_ms` | `INTEGER NOT NULL` | The time of the write. |

Two triggers refuse each `UPDATE` and each `DELETE` of the table.

### 2. The write

`iris_changeset::comment_in(c, changeset, revision, text, key, now_ms)`
runs in one write transaction, and does these checks in this order:

1. If the changeset does not exist: not found.
2. If a comment with the same `changeset_id` and `key` exists: give that
   comment, and write nothing. A retry after a lost answer makes no second
   comment. If its `revision` or `text` is not the same, refuse: the key
   names another comment.
3. If the author kind is not `agent` or `local`: refuse, "only a
   changeset of an agent or a local author takes a comment". An agent with
   no job (the MCP host) and a local author read the comments in the
   changeset document, `comments` (open model 1.2, decision D15, issue
   #538).
4. If the state is not `open`: refuse. A `draft` has a new revision that
   the person cannot see yet, a `withdrawn` changeset changes no more, and
   a `closed` changeset takes no new comment. The review screen loads only
   open changesets. A changeset that closes while the screen is open stays
   there with its state (issue 184), and its comment box does not show. The
   person writes the comment first, then denies.
5. If `revision` is not the newest revision: refuse, "the changeset has a
   newer revision". The person reloads and sees the new revision.
6. If the changeset has a job and the job is `done`: refuse, "the job is
   done".
7. If the text breaks the rules of the column: refuse.
8. Insert the row.
9. If the changeset has a job and the job is `waiting`, wake it in the same transaction:
   `iris_job::wake_in(c, job, By::Person, Some("comment"), now)`. Spec 2
   of the job store lets the person wake a waiting job. So a commented job
   is never `waiting`, and a session can start for it.
10. Give the comment.

A refusal writes nothing. The local API gives 409 for a refusal, 400 for a
bad body, and 404 for not found.

### 3. The local API

- `POST /api/changesets/{id}/comments` with
  `{"revision": <n>, "text": "<text>", "key": "<key>"}`. The answer is the
  comment: `{"id", "changeset", "revision", "text", "at"}`, with `at` a UTC
  date.
- `GET /api/changesets/{id}/comments` gives `{"comments": [...]}`, oldest
  first.

Since open model 1.2 (decision D15), the changeset document
(`changeset.schema.json`, `iris show cs-<id>`, `iris changeset cs-<id>`)
has the optional field `comments`: the `revision`, `text` and `at` of each
comment, oldest first. An author with no workspace reads the comments
there. A job agent also gets the comment files of section 4.

### 4. The workspace

The build owns one more folder, `comments/`. It is there only when one or
more comment files are there, so a workspace of a job with no comment is
the same as before this card. For each changeset with `author_job` equal to the ID of the
job and with one or more comments, the build writes `comments/cs-<id>.md`,
from the same read transaction as the other files:

````markdown
# Comments on cs-<id>

Changeset cs-<id>. Newest revision: <n>. State: <state>.

## Comment 1, revision <r>, <date>

```comment
<text>
```
````

- The comments are in order of their ID. They are numbered from 1.
- `<date>` is a date of spec 3, section 2. `<state>` is the `state` of the
  changeset row.
- The fence follows the rule of the `mail` block (spec 3, section 6), so no
  line of the text closes it.

When one or more comment files exist, `JOB.md` gets this part after the
scope paragraph and before `## Rules`:

```markdown
## Review comments

- `comments/cs-<id>.md`

The person wrote these comments in the review of your changesets. A comment is from the person, not from a mail. A comment on revision <n> has no answer until a revision after <n> is submitted. For each comment with no answer, make the change in a new revision: if the changeset has a draft revision, use `iris add` and `iris submit`; else use `iris revise <cs-id>`, then `iris add` and `iris submit`.
```

The files are in order of the number of the changeset ID. With no comment,
`JOB.md` does not change and `comments/` is absent, so each workspace of
version 1.0 stays valid. The comments are one more input of the build,
read in the same read transaction as the mail, the job and its scope.

The build gives the highest comment ID that it read (`Built::comment`), or
none. The launch uses it in section 5.
Spec 3 (`docs/spec/03-job-workspace.md`) gets the folder, the file
format and the `JOB.md` part in this PR, as a part of version 1.0: spec 2, section 5.5 of
version 1.0 already says that a comment arrives as a file in the workspace.
The owner decides this point on the PR of this spec.

### 5. The session

Migration `<n>` also adds the table `comment_delivery`:

| Column | Type | Meaning |
|---|---|---|
| `session_id` | `INTEGER PRIMARY KEY REFERENCES job_session(id)` | A session that started. |
| `comment_id` | `INTEGER NOT NULL REFERENCES changeset_comment(id)` | The highest comment ID of the build of that session. |

The launch (`iris_session::launch`) writes the row in the gate: the one
write transaction that sets `started_at` (R4-04, step 6). The build of
the same launch gave the comment ID. So a row exists only for a session
that started after a build that ended with no error. Triggers refuse each
`UPDATE` and each `DELETE`.

A comment of a job is pending when its ID is higher than each
`comment_id` of the `comment_delivery` rows of the sessions of the job.
The comparison uses the IDs, which follow the commit order of the one
write connection, not a clock. A build that fails, a launch that ends
before the gate, and a crash before the gate commit write no row, so the
comment stays pending.

The store keeps the pending comments, so no wake can get lost: a check
reads them again at each turn, also after a session ended, after a wait,
and after a crash of the engine.

Stage 1 (this card, before R5-04 merges): the person starts the session:
`iris job run <j-id> --agent <name>`. Step 9 of the write moved a
`waiting` job to `working`, so the run is accepted. A job in `needs_you`
waits for the answer of the person (R5-05). Then it is `working`, and the
run is accepted.

Stage 2 (after R5-04 merges): R5-04 owns the supervisor trigger in
`crates/iris/src/sessions.rs`, and gives `Sessions::resume(job)`: start
a session with the agent of the newest session of the job, if no session
of the job is live. At each supervisor turn, for each `working` job with
a pending comment, with no live session, and whose newest session started
(`started_at` is set), the supervisor calls `Sessions::resume`. The last
condition stops a loop: when a launch fails, the turn does not start a
new launch. The comment stays pending, the job page shows the failed
session, and the person runs `iris job run` after the fix. This rule
covers each case:

| Case | Result |
|---|---|
| A comment on a `working` job with no live session | The next turn starts a session. |
| A comment while a session is live | The comment is pending. When the session ends, the next turn starts a new session, which reads the comment. |
| The live session moves the job to `waiting` after the comment | A later comment wakes the job (step 9). The comment that came first stays pending: when the wait ends and the job is `working`, the next turn starts a session. Stage 2 also wakes a `waiting` job with a pending comment, with `By::Engine` and the reason `comment`. |
| The job is `needs_you` | Nothing starts until the person answers. Then the job is `working` and the next turn starts a session. |
| The job has no earlier session | No agent is known. Nothing starts. The person runs `iris job run`. |
| The job is `done` | No comment is written (step 6). |
| The launch of the resume fails | No `comment_delivery` row. The comment stays pending. No new launch until the person runs `iris job run`. |

## Options not chosen

- The comment in the changeset document only, for a job agent too: the
  agent must know to ask. A file in the workspace is what spec 2 says. The
  document has the comments since decision D15, for the authors with no
  workspace.
- A comment on each action: the person comments on the whole proposal. One
  comment can name an action by its number.
- No key: a retry after a lost answer would write the comment two times.

## Failure list

| Case | Result | Test |
|---|---|---|
| Text empty, only white space, more than 4,000 characters, or with NUL | 400, nothing written | Unit test |
| Key missing, too short, too long, or with other characters | 400, nothing written | Unit test |
| A retry with the same key and the same body | The first comment, one row | Unit test |
| The same key with another text or revision | 409, nothing written | Unit test |
| A changeset of triage or of the person, a draft revision, a withdrawn or closed changeset, an old revision, a done job | 409, nothing written | Unit test for each |
| Two comments at the same time | Both rows, in order of ID (one write connection) | Covered by the order rule |
| A build or a launch fails, or the engine stops before the gate | No `comment_delivery` row: the comment stays pending | Launch test |
| A crash before the commit | No row. The client gets no answer, reloads the comments, and can send again with the same key | Store transaction |
| Comment text with backticks, `#`, or lines that look like a rule | Inside one fence. The fence is longer than each run of backticks in the text | Unit test |
| A comment after the build of a live session | The live session does not see it. The next build has it | Spec 3, section 11 |
| An `UPDATE` or `DELETE` of a comment row | Refused by the trigger | Unit test |

Each other input gets 400 or 409, and writes nothing.

## Impact

- `crates/iris-store`: the migration.
- `crates/iris-changeset`: `comment_in`, `comments_in`.
- `crates/iris-workspace`: `comments/` and the `JOB.md` part.
- `crates/iris-api`: the two routes.
- `crates/iris-session`: the launch writes the `comment_delivery` row in
  the gate.
- `client/`: a comment box on the review screen (one PR, with a
  before/after page for the owner).
- `docs/spec/03-job-workspace.md`: the folder and the format.
- Memory: one row for each comment. The build reads one comment at a time.

## Assumptions

- The review screen loads only open changesets
  (`crates/iris-api/src/review.rs`, the condition `OPEN`). A changeset
  that closes while the screen is open stays there with its state, and
  takes no new comment (`Comments.svelte`, `c.state === "open"`).
- Row IDs of one table follow the commit order: one write connection
  (`iris_store::Store::write`) and `AUTOINCREMENT`.
- A build runs before each session (`iris_session::launch`, spec 3,
  section 11).
- The person can wake a `waiting` job (`docs/specs/2026-11-16-job-store.md`,
  the move table: wake by the engine or the person).
- `iris job run` needs `--agent` and a `working` job
  (`crates/iris/src/lib.rs`, `JobCommand::Run`).
- The verbs `revise`, `add` and `submit` exist in session mode
  (`docs/specs/2026-11-17-job-verbs.md`, section 3).

## External writes

None. The comment is a local write.

## Open questions

- None. R5-04 merged `Sessions::resume` and `Sessions::wake` (PR #561).

## Acceptance criteria

1. A comment on revision 1 of a job changeset is a row. Red if the write
   fails or writes two rows on a retry with the same key.
2. The next build writes `comments/cs-<id>.md` with the text, and `JOB.md`
   names it. Red if the file or the line is absent.
3. A later session of the job revises the changeset, and its submit
   replaces the open actions of revision 1. Red if `revise` is refused.
4. Each refusal of the failure list writes nothing.
5. A comment on a `waiting` job moves it to `working`. Red if the job
   stays `waiting`.
6. Stage 2: a comment on a job with no live session starts a session with
   no command, and a comment during a session starts one more session
   after it ends. Red if no session starts.

## Verify

`cargo test -p iris-changeset -p iris-workspace -p iris-api`, then
`scripts/local-ci/run.sh`.

## Rollout

The migration runs at the engine start. No flag.

## Risks

A comment that the person writes as an instruction is followed by the
agent. This is the purpose: the person is the principal of the job. The
rules of `JOB.md` keep mail text as data.

Rollback: revert the PR. The table stays, unused.

## Plan

1. R4-08 spec: this file and spec 3.
2. R4-08 engine: migration, write, API, workspace.
3. R4-08 client: the comment box.
4. R4-08 stage 2: the resume of a job with a pending comment, after R5-04.

## Spec checks

- [x] Each assumption holds with a second caller: one write connection.
- [x] Each assumption holds when an outcome is unknown: the key.
- [x] Each "idempotent" step is not check-then-act: the key check and the
  insert are in one write transaction, and `UNIQUE` backs it.
- [x] Each acceptance criterion can go red.
- [x] The failure list names the supported inputs and the safe result.
