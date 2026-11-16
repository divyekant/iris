-- Migration 12: the job store (card R4-01, docs/specs/2026-11-16-job-store.md).
--
-- A job is a goal with limits for an agent. `job` holds one row for each
-- job, and `job_event` one row for each state change. The event moves the
-- job: the trigger job_event_move checks the move against the move table of
-- the spec, and job_event_apply copies the state to the job row. The
-- trigger job_row refuses each other change of the row, so the state of a
-- job always agrees with its newest event.
--
-- The triggers hold for each SQL path. They are not a security boundary
-- (issue #158): an agent session has no SQL access.

CREATE TABLE job (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  goal TEXT NOT NULL CHECK (length(goal) BETWEEN 1 AND 2000),
  -- The limit date of the goal, in milliseconds. The engine does not move a
  -- job on this date (issue #248).
  due_at INTEGER,
  created_at INTEGER NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('working', 'waiting', 'needs_you', 'done')),
  -- No foreign key: the cleanup of gone mail can delete a thread row.
  wait_thread INTEGER,
  wait_until INTEGER,
  question TEXT CHECK (length(question) BETWEEN 1 AND 2000),
  end_kind TEXT CHECK (end_kind IN ('complete', 'cancel')),
  -- The time of the newest event.
  updated_at INTEGER NOT NULL,
  CHECK ((state = 'waiting') = (wait_until IS NOT NULL)),
  CHECK (wait_thread IS NULL OR wait_until IS NOT NULL),
  CHECK ((state = 'needs_you') = (question IS NOT NULL)),
  CHECK ((state = 'done') = (end_kind IS NOT NULL))
);

CREATE INDEX job_state ON job(state);

CREATE TABLE job_event (
  -- The order of the events.
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  job_id INTEGER NOT NULL REFERENCES job(id),
  at INTEGER NOT NULL,
  -- The state after the event.
  state TEXT NOT NULL CHECK (state IN ('working', 'waiting', 'needs_you', 'done')),
  by TEXT NOT NULL CHECK (by IN ('person', 'agent', 'engine')),
  wait_thread INTEGER,
  wait_until INTEGER,
  question TEXT CHECK (length(question) BETWEEN 1 AND 2000),
  answer TEXT CHECK (length(answer) BETWEEN 1 AND 10000),
  end_kind TEXT CHECK (end_kind IN ('complete', 'cancel')),
  why TEXT CHECK (length(why) BETWEEN 1 AND 2000),
  CHECK ((state = 'waiting') = (wait_until IS NOT NULL)),
  CHECK (wait_thread IS NULL OR wait_until IS NOT NULL),
  CHECK ((state = 'needs_you') = (question IS NOT NULL)),
  CHECK ((state = 'done') = (end_kind IS NOT NULL)),
  CHECK (answer IS NULL OR state = 'working')
);

-- The events of one job, in order.
CREATE INDEX job_event_job ON job_event(job_id, id);

-- A new job is in `working`, with no wait, no question and no end. A
-- BEFORE trigger runs before INSERT OR REPLACE deletes a row, so no insert
-- replaces a job.
CREATE TRIGGER job_insert BEFORE INSERT ON job
WHEN new.state <> 'working' OR new.wait_thread IS NOT NULL OR new.wait_until IS NOT NULL
  OR new.question IS NOT NULL OR new.end_kind IS NOT NULL
  OR new.updated_at IS NOT new.created_at
  OR EXISTS (SELECT 1 FROM job WHERE id = new.id)
BEGIN
  SELECT RAISE(ABORT, 'job: a new job is in working, with no wait, question or end');
END;

-- The create event, in the same statement as the job row. So no commit
-- holds a job with no event.
CREATE TRIGGER job_create AFTER INSERT ON job
BEGIN
  INSERT INTO job_event (job_id, at, state, by) VALUES (new.id, new.created_at, 'working', 'person');
END;

-- The move table of the spec. A job with no event is only a job in the
-- statement of its create: then the only event is the create event. Each
-- other job has an event, so an insert of a second create event is a move
-- from the current state, and the table has no move to the same state.
-- The time of an event is not before the newest event of its job.
CREATE TRIGGER job_event_move BEFORE INSERT ON job_event
WHEN NOT EXISTS (SELECT 1 FROM job WHERE id = new.job_id)
  OR EXISTS (SELECT 1 FROM job_event WHERE id = new.id)
  OR new.at < (SELECT updated_at FROM job WHERE id = new.job_id)
  OR NOT (
    CASE WHEN NOT EXISTS (SELECT 1 FROM job_event WHERE job_id = new.job_id) THEN
      -- create
      new.state = 'working' AND new.by = 'person'
      AND new.at = (SELECT created_at FROM job WHERE id = new.job_id)
      AND new.answer IS NULL AND new.why IS NULL
    ELSE
      CASE (SELECT state FROM job WHERE id = new.job_id)
        WHEN 'working' THEN
          -- wait
          (new.state = 'waiting' AND new.by = 'agent' AND new.wait_until > new.at
           AND new.question IS NULL AND new.answer IS NULL AND new.end_kind IS NULL)
          -- ask
          OR (new.state = 'needs_you' AND new.by = 'agent'
              AND new.wait_until IS NULL AND new.answer IS NULL AND new.end_kind IS NULL)
          -- finish: the agent completes, the person completes or cancels
          OR (new.state = 'done' AND new.wait_until IS NULL AND new.question IS NULL
              AND new.answer IS NULL
              AND (new.by = 'person' OR (new.by = 'agent' AND new.end_kind = 'complete')))
        WHEN 'waiting' THEN
          -- wake
          (new.state = 'working' AND new.by IN ('engine', 'person')
           AND new.wait_until IS NULL AND new.question IS NULL AND new.answer IS NULL
           AND new.end_kind IS NULL)
          OR (new.state = 'done' AND new.by = 'person'
              AND new.wait_until IS NULL AND new.question IS NULL AND new.answer IS NULL)
        WHEN 'needs_you' THEN
          -- answer
          (new.state = 'working' AND new.by = 'person' AND new.answer IS NOT NULL
           AND new.wait_until IS NULL AND new.question IS NULL AND new.end_kind IS NULL)
          OR (new.state = 'done' AND new.by = 'person'
              AND new.wait_until IS NULL AND new.question IS NULL AND new.answer IS NULL)
        -- done is final
        ELSE 0
      END
    END
  )
BEGIN
  SELECT RAISE(ABORT, 'job: this move is not in the move table');
END;

-- The event moves the job row. An event with an explicit ID lower than an
-- earlier event is refused, so the newest event is the last one.
CREATE TRIGGER job_event_apply AFTER INSERT ON job_event
BEGIN
  SELECT RAISE(ABORT, 'job: an event is the newest event')
  WHERE new.id <> (SELECT max(id) FROM job_event);
  UPDATE job SET state = new.state, wait_thread = new.wait_thread,
    wait_until = new.wait_until, question = new.question, end_kind = new.end_kind,
    updated_at = new.at
  WHERE id = new.job_id;
END;

-- The goal, the limits and the time of a job do not change. The state and
-- its values are always the values of the newest event.
CREATE TRIGGER job_row BEFORE UPDATE ON job
WHEN new.id IS NOT old.id OR new.goal IS NOT old.goal OR new.due_at IS NOT old.due_at
  OR new.created_at IS NOT old.created_at
  OR NOT EXISTS (
    SELECT 1 FROM (SELECT * FROM job_event WHERE job_id = old.id ORDER BY id DESC LIMIT 1) e
    WHERE e.state = new.state AND e.wait_thread IS new.wait_thread
      AND e.wait_until IS new.wait_until AND e.question IS new.question
      AND e.end_kind IS new.end_kind AND e.at = new.updated_at)
BEGIN
  SELECT RAISE(ABORT, 'job: only a new event changes a job');
END;

CREATE TRIGGER job_delete BEFORE DELETE ON job
BEGIN
  SELECT RAISE(ABORT, 'job: a job is never deleted');
END;

CREATE TRIGGER job_event_update BEFORE UPDATE ON job_event
BEGIN
  SELECT RAISE(ABORT, 'job: an event does not change');
END;

CREATE TRIGGER job_event_delete BEFORE DELETE ON job_event
BEGIN
  SELECT RAISE(ABORT, 'job: an event is never deleted');
END;
