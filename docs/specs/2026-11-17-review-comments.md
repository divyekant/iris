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

- A comment on a changeset with no job (an agent of the MCP host, triage,
  `local`). These authors have no workspace. The engine refuses the
  comment. A limit issue records it.
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
3. If `author_job` is NULL: refuse, "only a changeset of a job takes a
   comment".
4. If the state is `draft`: refuse, "a new revision is a draft". If the
   state is `withdrawn`: refuse. The states `open` and `closed` take a
   comment, because the person can deny each action and then comment.
5. If `revision` is not the newest revision: refuse, "the changeset has a
   newer revision". The person reloads and sees the new revision.
6. If the job is `done`: refuse, "the job is done".
7. If the text breaks the rules of the column: refuse.
8. Insert the row, and give the comment.

A refusal writes nothing. The local API gives 409 for a refusal, 400 for a
bad body, and 404 for not found.

### 3. The local API

- `POST /api/changesets/{id}/comments` with
  `{"revision": <n>, "text": "<text>", "key": "<key>"}`. The answer is the
  comment: `{"id", "changeset", "revision", "text", "at"}`, with `at` a UTC
  date.
- `GET /api/changesets/{id}/comments` gives `{"comments": [...]}`, oldest
  first.

The changeset document of the open model (`changeset.schema.json`) does not
change.

### 4. The workspace

The build owns one more folder, `comments/`. It is always there, also when
it is empty. For each changeset with `author_job` equal to the ID of the
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

The person wrote these comments in the review of your changesets. A comment is from the person, not from a mail. For each changeset with a comment on its newest revision, make a new revision: `iris revise <cs-id>`, then `iris add` and `iris submit`.
```

The files are in order of the number of the changeset ID. With no comment,
`JOB.md` does not change, so each workspace of version 1.0 stays valid.
Spec 3 gets this section, as a part of version 1.0: spec 2, section 5.5 of
version 1.0 already says that a comment arrives as a file in the workspace.
The owner decides this point on the PR of this spec.

### 5. The session (stage 2, after R5-04)

R5-04 owns the supervisor trigger in `crates/iris/src/sessions.rs`. Its
thread gives two entry points: `iris_job::wake_in(c, job, By::Engine,
Some("comment"), now)` for a `waiting` job, and `Sessions::resume(job)`,
which starts a session with the agent of the last session when no session
of the job is live. Stage 2 does this:

- The comment write wakes a `waiting` job in the same transaction. The
  start path of R5-04 starts the session.
- Each supervisor turn finds each `working` job with no live session that
  has a comment newer than the start of its last session, and calls
  `Sessions::resume`. A job with no earlier session has no agent to start,
  and the turn skips it.

Until stage 2 merges, the person starts the session with
`iris job run <j-id>`. The new session gets a new build, so it reads the
comment.

## Options not chosen

- The comment in the changeset document (`iris show cs-<id>`): a change of
  `changeset.schema.json`, and the agent must know to ask. A file in the
  workspace is what spec 2 says.
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
| A changeset with no job, a draft revision, a withdrawn changeset, an old revision, a done job | 409, nothing written | Unit test for each |
| Two comments at the same time | Both rows, in order of ID (one write connection) | Covered by the order rule |
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
- `client/`: a comment box on the review screen (one PR, with a
  before/after page for the owner).
- `docs/spec/03-job-workspace.md`: the folder and the format.
- Memory: one row for each comment. The build reads one comment at a time.

## Assumptions

- `revise_in` takes a changeset in the state `closed`
  (`crates/iris-changeset/src/lib.rs`, `revise_in` checks only `withdrawn`
  and `submitted_at`).
- A build runs before each session (`iris_session::launch`, spec 3,
  section 11).
- The verbs `revise`, `add` and `submit` exist in session mode
  (`docs/specs/2026-11-17-job-verbs.md`, section 3).

## External writes

None. The comment is a local write.

## Open questions

- The exact names of the R5-04 entry points (stage 2).

## Acceptance criteria

1. A comment on revision 1 of a job changeset is a row. Red if the write
   fails or writes two rows on a retry with the same key.
2. The next build writes `comments/cs-<id>.md` with the text, and `JOB.md`
   names it. Red if the file or the line is absent.
3. A later session of the job revises the changeset, and its submit
   replaces the open actions of revision 1. Red if `revise` is refused.
4. Each refusal of the failure list writes nothing.
5. Stage 2: a comment on a waiting job starts a session with no command.

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
4. R4-08 stage 2: the wake and the resume, after R5-04.

## Spec checks

- [x] Each assumption holds with a second caller: one write connection.
- [x] Each assumption holds when an outcome is unknown: the key.
- [x] Each "idempotent" step is not check-then-act: the key check and the
  insert are in one write transaction, and `UNIQUE` backs it.
- [x] Each acceptance criterion can go red.
- [x] The failure list names the supported inputs and the safe result.
