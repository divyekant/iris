-- Migration 50: the edit of a job after its start, and the request ID of a
-- start from a typed goal (decision D25,
-- docs/specs/2026-11-24-job-goal-and-scope.md, issue #384).
--
-- `job_edit` keeps each change of the goal and the due date, with the
-- values before and after. The trigger job_edit_apply copies the values
-- after the edit to the job row. The trigger job_row of migration 14 is made
-- again: it allows a change of the goal or the due date only to the values
-- of the newest edit of the job. Each other rule of job_row stays.
--
-- IF EXISTS and IF NOT EXISTS: a test store that sets an older
-- user_version runs this migration again.

CREATE TABLE IF NOT EXISTS job_edit (
  id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
  job_id INTEGER NOT NULL REFERENCES job(id),
  at INTEGER NOT NULL CHECK (typeof(at) = 'integer' AND at BETWEEN 0 AND 253402300799999),
  goal_before TEXT NOT NULL,
  due_before INTEGER,
  goal TEXT NOT NULL CHECK (typeof(goal) = 'text' AND instr(goal, char(0)) = 0 AND length(goal) BETWEEN 1 AND 2000),
  due_at INTEGER CHECK (due_at IS NULL OR typeof(due_at) = 'integer' AND due_at BETWEEN 0 AND 253402300799999)
);

CREATE INDEX IF NOT EXISTS job_edit_job ON job_edit(job_id, id);

-- An edit of an open job, not before the newest edit, from the values of
-- the job row now, that changes a value.
CREATE TRIGGER IF NOT EXISTS job_edit_insert BEFORE INSERT ON job_edit
WHEN NOT EXISTS (SELECT 1 FROM job WHERE id = new.job_id AND state <> 'done')
  OR new.at < coalesce((SELECT max(at) FROM job_edit WHERE job_id = new.job_id), 0)
  OR NOT EXISTS (SELECT 1 FROM job WHERE id = new.job_id
                 AND goal = new.goal_before AND due_at IS new.due_before)
  OR (new.goal = new.goal_before AND new.due_at IS new.due_before)
BEGIN
  SELECT RAISE(ABORT, 'job: an edit changes the goal or the due date of an open job');
END;

CREATE TRIGGER IF NOT EXISTS job_edit_apply AFTER INSERT ON job_edit
BEGIN
  SELECT RAISE(ABORT, 'job: an edit is the newest edit')
  WHERE new.id <> (SELECT max(id) FROM job_edit);
  UPDATE job SET goal = new.goal, due_at = new.due_at WHERE id = new.job_id;
END;

CREATE TRIGGER IF NOT EXISTS job_edit_update BEFORE UPDATE ON job_edit
BEGIN
  SELECT RAISE(ABORT, 'job: an edit does not change');
END;

CREATE TRIGGER IF NOT EXISTS job_edit_delete BEFORE DELETE ON job_edit
BEGIN
  SELECT RAISE(ABORT, 'job: an edit is never deleted');
END;

DROP TRIGGER IF EXISTS job_row;

-- The goal and the due date change only to the values of the newest edit.
-- The limits of time and ID do not change. The state and its values are
-- always the values of the newest event.
CREATE TRIGGER job_row BEFORE UPDATE ON job
WHEN new.id IS NOT old.id OR new.created_at IS NOT old.created_at
  OR ((new.goal IS NOT old.goal OR new.due_at IS NOT old.due_at) AND NOT EXISTS (
    SELECT 1 FROM (SELECT * FROM job_edit WHERE job_id = old.id ORDER BY id DESC LIMIT 1) d
    WHERE d.goal = new.goal AND d.due_at IS new.due_at))
  OR NOT EXISTS (
    SELECT 1 FROM (SELECT * FROM job_event WHERE job_id = old.id ORDER BY id DESC LIMIT 1) e
    WHERE e.state = new.state AND e.wait_thread IS new.wait_thread
      AND e.wait_until IS new.wait_until AND e.question IS new.question
      AND e.end_kind IS new.end_kind AND e.at = new.updated_at)
BEGIN
  SELECT RAISE(ABORT, 'job: only a new event or a new edit changes a job');
END;

-- The request ID of a start from a typed goal: a retry with the same ID
-- finds the job of the first start.
CREATE TABLE IF NOT EXISTS job_request (
  request TEXT PRIMARY KEY CHECK (typeof(request) = 'text' AND length(request) BETWEEN 1 AND 64),
  job_id INTEGER NOT NULL UNIQUE REFERENCES job(id)
) WITHOUT ROWID;
