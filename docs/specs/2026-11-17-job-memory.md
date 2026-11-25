# Spec: the memory of a job between sessions

- Date: 2026-11-17
- Card: R5-06
- Status: Built

## Problem

An agent of a job keeps its memory in `notes.md` of the job workspace
(`docs/spec/03-job-workspace.md`, section 3). The workspace keeps the file
between sessions, so a job that waits (R5-04, `docs/specs/2026-11-17-wait.md`)
finds its notes at the wake.

The file lives only in the workspace folder, `<data dir>/workspaces/j-<id>`.
The backup of the data folder (`Store::backup`, specification 1, section 5.1)
copies `iris.db`, the content files, the key and `policy.toml`. It does not
copy the workspaces (issue #350). So these events lose the memory of each job:

- A restore from a backup.
- A move of the data folder to a new computer with the backup.
- A person removes a workspace folder to free disk space.

A job that waits 3 days and wakes after one of these events starts with no
notes. The agent does the work again, or asks the person again.

## Goal

1. At the start of each session, `notes.md` holds the same bytes that the
   agent left at the end of the last session of the job.
2. The store keeps a copy of the notes of each job. The backup holds the
   copy, because the copy is in `iris.db`.
3. If `notes.md` is not in the workspace, the build writes the copy of the
   store, byte for byte.

## Not in scope

- Show the notes to the person in the web client or the job page.
- A history of the notes: the store keeps the last copy only.
- A backup of `drafts/`. `iris backup` copies `drafts/` of each workspace
  (issue #350, specification 1 section 5.1).
- A change to `iris job` verbs or to the session socket.

## Design

### The store

Migration `N` (the next free number at merge) adds one table:

```sql
CREATE TABLE job_notes (
  job_id   INTEGER PRIMARY KEY REFERENCES job(id),
  text     BLOB NOT NULL CHECK (typeof(text) = 'blob' AND length(text) <= 65536),
  saved_ms INTEGER NOT NULL
);

-- The copy ends with the job, in the transaction of the move to done.
CREATE TRIGGER job_notes_end AFTER INSERT ON job_event
WHEN NEW.state = 'done'
BEGIN
  DELETE FROM job_notes WHERE job_id = NEW.job_id;
END;

-- A done job gets no copy. The move to done cannot go back.
CREATE TRIGGER job_notes_done BEFORE INSERT ON job_notes
WHEN (SELECT state FROM job WHERE id = NEW.job_id) = 'done'
BEGIN
  SELECT RAISE(IGNORE);
END;
```

SQLite runs the BEFORE INSERT trigger on each upsert, also on one that
takes the UPDATE path. The trigger checks the state of the job, not the
row, so it ignores each save of a done job on both paths. A done job also
has no row: the first trigger deleted it.

- One row for each job, at most. A new save replaces the row (`INSERT ...
  ON CONFLICT(job_id) DO UPDATE`).
- `text` is a BLOB: the store keeps the bytes as they are. Iris does not
  check UTF-8 and does not change line ends.
- 65,536 bytes (64 KiB) is the limit, `iris_job::NOTES_MAX` (issue #563).
- Only the move of the job to `done` deletes its row, by the trigger
  `job_notes_end`. Each path to `done` (the finish of the agent, the
  finish and the cancel of the person) inserts a `job_event` with the state
  `done`, so the trigger covers each path by construction.

`iris-job` gets two functions: `notes(store, job) -> Option<Vec<u8>>` and
`save_notes(store, job, &[u8], now) -> bool`. `save_notes` refuses a text
over the limit with `Error::Refused` and writes nothing. It gives `false`
when the trigger ignored the write (a done job).

### Save the notes (`iris_workspace::save_notes`)

`save_notes(store, job, dir, now) -> Result<Notes>` reads `<dir>/notes.md`
and saves it in the store. The result:

| Result | When | Store |
|---|---|---|
| `Notes::Saved` | A regular file of 64 KiB or less. | The copy is the bytes of the file. |
| `Notes::Absent` | No `notes.md`. | No change. |
| `Notes::NotAFile` | A link, a folder, a FIFO, a socket or a device. | No change. |
| `Notes::TooLarge` | A regular file of more than 64 KiB. | No change. |
| `Notes::Done` | The job is done. | No copy (the trigger ignores the write). |
| `Notes::Unreadable` | Another I/O error of the open or the read (for example, the agent removed the read permission). | No change. |

The read:

1. Open `notes.md` with `O_NOFOLLOW` and `O_NONBLOCK`. A link fails to
   open (`ELOOP`): `NotAFile`. A FIFO opens and does not block.
2. Check the file type with `fstat` on the open file, not on the path:
   not a regular file gives `NotAFile`.
3. Read at most 65,537 bytes. More than 65,536 gives `TooLarge`.
4. Save the bytes in one write transaction.

`read_notes(dir)` does steps 1 to 3 with no store, for the reap. A store
error is `Err`. Memory:
one buffer of at most 64 KiB + 1.

### When Iris saves the notes

The notes are a file that the agent changes while its session runs. Iris
reads the file only while no process of the job can write it.

1. **At each build** (`iris_workspace::build`). The build runs only while no
   session of the job runs (section 11 of spec 3). Before it makes the agent
   paths, the build calls `save_notes`. The result does not stop the build,
   but an `Err` does.
2. **At each reap**, in the reap write (`iris_session::reap::reap_check`).
   The reap check confirms rules 1 to 3: the helper ended, the group of
   the agent is empty, and the isolation holds no process of the session.
   Then no process of the job runs, and the session is still live in the
   store (`reaped_at` is NULL), so `Sessions::run` refuses a new session of
   the job. The reap check reads `notes.md` now (`iris_workspace::read_notes`),
   and writes the copy and `reaped_at` in **one write transaction**
   (`iris_job::reap_with_notes`). So:

   - No new session can write the file during the read: the job has a live
     session until the transaction commits.
   - No backup and no crash can see a reaped session without the save of
     its notes: the backup reads a committed state (`VACUUM INTO`), and a
     crash keeps the old state or the new state.

   A `TooLarge`, `NotAFile` or `Unreadable` result reaps with no save, and
   the engine writes one line in its log with the job ID. The reap does not
   fail on the notes, so an agent cannot keep its session live with a bad
   `notes.md`. A store error fails the reap write, as now, and the next
   turn tries the reap again.

The save at the reap keeps the copy current while a job waits. So a backup
taken during a wait of 3 days holds the notes of the last session. A backup
taken while a session runs holds the copy of the earlier save: the store
saves the notes only while no process of the job runs.

### Restore the notes

The build makes `notes.md` when it is not there (spec 3, section 3). The
change: the new file holds the copy of the store if one exists. Else it
holds `# Notes` and a line end, as now.

The build writes the restored file beside the workspace first (mode `0600`,
`create_new`), syncs it, then links it to `notes.md` (issue #604). A link at `notes.md` stays as it is (`exists` is true for a
link), as now.

### The rules for the agent

The rule line in `JOB.md` changes to:

```
- Keep your notes in `notes.md`. Iris keeps the file between sessions. Keep it at 64 KiB or less.
```

Spec 3 changes in the same PR as the code: section 3 (the build reads
`notes.md` and can make it from the copy), the rules of section 9, and the
sample of `JOB.md`.

### Remove the notes

The workspace spec (`docs/specs/2026-11-16-job-workspace.md`, "Text that
the build does not check") lets the person delete the workspace folder to
remove the agent files, for example notes that quote mail of an ended
grant. With this design, the build writes the copy back. So the rules
change:

- **A done job.** A done job gets no new session, so its notes have no
  more use. The trigger `job_notes_end` deletes the copy in the
  transaction of the move to `done`, on each path to `done`, also when the
  person deleted the workspace folder first. No later save writes a new
  copy (`job_notes_done`).
- **An open job** (working, waiting or needs you). The person edits
  `notes.md` while no session of the job runs. The next build or reap
  saves the edit, and the store keeps no older copy. A deleted folder does
  not remove the notes of an open job: the build of the next session
  writes the copy back. The person can stop that session, edit `notes.md`
  and run the job again.

For an open job whose folder is gone, no verb clears the copy before the
next session. This is a known limit, with an issue in the code PR.

## Options not chosen

- Keep the notes in the store only, and write the file new at each build.
  The agent then loses a change to the notes when the reap save fails (for
  example, a file over the limit). The file stays the memory; the store holds
  a copy.
- Add `workspaces/` to the backup. The backup then copies mail text of each
  workspace again (the threads are already in the store) and the files of
  `drafts/`. A backup must stay the size of the store.
- A verb to clear the copy (`iris job forget-notes`). An edit of the file
  does the same work with no new verb.
- Cut a file over the limit to 64 KiB. A cut note can mislead the agent at a
  restore. The design keeps the last good copy.
- Save at each write of the agent (a file watch). Each platform has its own
  watch API (inotify, FSEvents), and a read during a write can see half a
  file.
- Libraries: the read uses only `std` and the `libc` constants `O_NOFOLLOW`
  and `O_NONBLOCK`, which macOS and Linux both have. No parser applies: Iris
  does not read the content of the notes.

## Failure list

Supported input: a regular file `notes.md` of 0 to 65,536 bytes, any bytes.
The safe result for all other input: the store keeps its last copy, the file
stays as it is, and the session starts.

| Case | Result | Test |
|---|---|---|
| A regular file, any bytes (not UTF-8, no line end, NUL bytes), 0 to 64 KiB. | The copy is the same bytes. | Property test: random bytes and sizes. |
| A file of 64 KiB + 1 or more. | `TooLarge`. The copy stays. The file stays. | Unit test at 65,536 and 65,537 bytes. |
| A link to a file in or out of the workspace. | `NotAFile`. Iris reads nothing through the link. | Unit test. |
| A FIFO. | `NotAFile`, with no block. | Unit test (Unix). |
| A folder. | `NotAFile`. | Unit test. |
| No `notes.md`, and a copy in the store. | The build writes the copy, byte for byte. | Unit test. |
| No `notes.md`, and no copy. | The build writes `# Notes`. | Existing test. |
| A restore stops partway (a full disk, a crash). | No `notes.md` is there: the build writes the bytes to `.<workspace>.notes` beside the workspace, syncs them, then makes `notes.md` with a hard link, which never replaces a file. The next build removes the old part and writes again, so it never saves a part over the copy (issue #604). | Unit test `a_restore_that_stops_partway_leaves_no_file`. |
| The job moves to `done` by any path, with or without its folder. | The copy is deleted in the same transaction. A later save writes nothing, and a build writes `# Notes`. | Unit test: finish by the agent, finish and cancel by the person. |
| The engine crashes during the reap write. | The transaction keeps the session live with the old copy, or reaped with the new copy. The next engine start reaps the session again. | SQLite; the existing restart test of R4-04. |
| A backup runs during a reap. | The backup holds the state before or after the reap transaction, never a reaped session without its save. | Review: one transaction. |
| `notes.md` has no read permission. | `Unreadable`. The reap and the build go on. The copy stays. | Unit test. |
| The workspace folder is gone, and the job wakes. | The launch build makes the folder and restores the notes. | Engine test with the fake clock. |
| The agent changes the file after the reap and before the build. | Not possible: no process of the job runs between a reap and the next launch. | The reap check (R4-04). |
| A new session of the job starts during the reap read. | Not possible: the reaped session is live until the reap transaction commits, and `run` refuses a job with a live session. | Engine test: run during a reap is refused (existing R4-04 rule). |
| A crash during the save. | The transaction gives the old copy or the new copy. | SQLite. |
| A store error at the build save. | The build stops with an error. No session starts. | Unit test with a read-only store, if the build can make one; else review. |

## Impact

- `crates/iris-store/src/schema_<N>.sql`: the table `job_notes`.
- `crates/iris-job`: `NOTES_MAX`, `notes`, `save_notes`.
- `crates/iris-workspace`: `save_notes`, the restore in `build`, the rule
  line. New dependency: `libc` (already in `Cargo.lock`).
- `crates/iris-session/src/reap.rs`: the read of the notes and the reap write with the notes.
- `crates/iris/src/sessions.rs`: one log line for a reap with no save.
- `docs/spec/03-job-workspace.md`: sections 3, 9 and the sample.
- Memory: one buffer of at most 64 KiB + 1 during a save. Disk: at most
  64 KiB in `iris.db` for each job.

## Assumptions

- The backup copies `iris.db` and not `workspaces/`:
  `crates/iris-store/src/backup.rs`, `Store::backup`.
- A build runs only while no session of the job runs:
  `docs/spec/03-job-workspace.md`, section 11, and
  `iris_session::launch` (the launch builds before the helper starts).
- After the reap check, no process of the session runs:
  `iris_session::reap::reap_check` (rules 1 to 3).
- `Sessions::run` refuses a job with a live session (`reaped_at` is NULL):
  `crates/iris/src/sessions.rs`, `live_of`.
- The backup copies a committed state of `iris.db` (`VACUUM INTO`):
  `crates/iris-store/src/backup.rs`.
- `O_NOFOLLOW` and `O_NONBLOCK` exist on macOS and Linux: POSIX.1-2008.

## External writes

None. The save writes only the store and the build writes only the
workspace.

## Open questions

- None.

## Acceptance criteria

1. A job waits, its workspace folder is removed, and the job wakes: the new
   session finds `notes.md` with the same bytes. Red if the file is absent,
   holds `# Notes`, or differs by one byte.
2. A job waits 3 days and wakes (the R5-04 test): `notes.md` is byte-equal
   to the file at the end of the last session. Red if it differs.
3. A `notes.md` of 64 KiB + 1 does not replace the copy of the store. Red if
   the store copy changes or the session does not start.
4. A link at `notes.md` to a file out of the workspace: the store copy does
   not hold the bytes of the target. Red if it does.

## Verify

```bash
cargo test -p iris-job notes
cargo test -p iris-workspace --test notes
cargo test -p iris --test engine_sessions -- notes
```

## Rollout

The migration adds an empty table. The first save of each job comes at its
next reap or build. A job with no session since the update has no copy until
then.

## Risks

- A store with many jobs grows by up to 64 KiB for each job. Rollback: the
  table can stay; an older program does not open a newer schema, so a
  rollback needs a restore of an older backup.

## Plan

1. R5-06: this spec, then one code PR (migration, `iris-job`,
   `iris-workspace`, the reap save, spec 3).

## Test plan

See the failure list and the acceptance criteria.

## Spec checks

- [x] Each assumption holds with a second caller: two builds of one workspace
  take the build lock; a reap read and a launch of one job exclude each other,
  because the session is live until the reap transaction commits.
- [x] Each assumption holds when an outcome is unknown: the save and the reap
  are one transaction, so a crash or a backup sees both or neither.
- [x] Each dropped mechanism was dropped for a reason (Options not chosen).
- [x] No "idempotent" step is check-then-act: the save is one upsert.
- [x] Each property has a test (failure list).
- [x] The hardest problem is a read of a file that the agent can change; the
  design reads only while no process of the job runs, through no link.
- [x] Each acceptance criterion can go red.
- [x] The assumptions do not contradict each other.
- [x] The failure list names the supported input and the safe result.
