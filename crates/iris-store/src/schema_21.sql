-- Migration 21: the sessions of a job (card R4-04b,
-- docs/specs/2026-11-17-session-launcher.md).
--
-- One row for each session of an agent on a job. A session has two ends:
-- `ended_at` is the end of the grant (the session socket answers nothing
-- after it), and `reaped_at` is the end of the processes (the reap check of
-- the engine passed). An open session has `ended_at IS NULL`. A live session
-- has `reaped_at IS NULL`. A row is never deleted.

CREATE TABLE job_session (
  -- AUTOINCREMENT: a session ID is never used again. The ID se-<id> needs a
  -- positive ID.
  id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
  job_id INTEGER NOT NULL REFERENCES job(id),
  agent TEXT NOT NULL CHECK (typeof(agent) = 'text' AND length(agent) BETWEEN 1 AND 64
    AND agent NOT GLOB '*[^a-z0-9-]*'),
  registered_at INTEGER NOT NULL CHECK (typeof(registered_at) = 'integer'
    AND registered_at BETWEEN 0 AND 253402300799999),
  -- A process ID of 0 or 1 is never valid here: kill(0) and kill(-1) signal
  -- other processes than the session.
  helper_pid INTEGER CHECK (helper_pid IS NULL OR (typeof(helper_pid) = 'integer'
    AND helper_pid BETWEEN 2 AND 2147483647)),
  agent_pgid INTEGER CHECK (agent_pgid IS NULL OR (typeof(agent_pgid) = 'integer'
    AND agent_pgid BETWEEN 2 AND 2147483647)),
  started_at INTEGER CHECK (started_at IS NULL OR (typeof(started_at) = 'integer'
    AND started_at BETWEEN 0 AND 253402300799999)),
  ended_at INTEGER CHECK (ended_at IS NULL OR (typeof(ended_at) = 'integer'
    AND ended_at BETWEEN 0 AND 253402300799999)),
  end_reason TEXT CHECK (end_reason IS NULL OR end_reason IN ('exit', 'stop', 'revoke',
    'job_state', 'time_limit', 'engine', 'launch_failed', 'helper_failed')),
  exit_code INTEGER CHECK (exit_code IS NULL OR (typeof(exit_code) = 'integer'
    AND exit_code BETWEEN 0 AND 255)),
  reaped_at INTEGER CHECK (reaped_at IS NULL OR (typeof(reaped_at) = 'integer'
    AND reaped_at BETWEEN 0 AND 253402300799999)),
  CHECK ((ended_at IS NULL) = (end_reason IS NULL)),
  CHECK (exit_code IS NULL OR end_reason = 'exit'),
  CHECK (reaped_at IS NULL OR ended_at IS NOT NULL),
  CHECK (agent_pgid IS NULL OR helper_pid IS NOT NULL)
);

-- One live session for each job. A second register fails until the
-- processes of the first session stopped.
CREATE UNIQUE INDEX job_session_live ON job_session(job_id) WHERE reaped_at IS NULL;

-- A BEFORE trigger runs before INSERT OR REPLACE deletes a row, so the ID
-- check stops a replace. An insert is a register: a job in `working` with
-- no live session, and NULL in each column that a move sets.
CREATE TRIGGER job_session_insert BEFORE INSERT ON job_session
WHEN EXISTS (SELECT 1 FROM job_session WHERE id = new.id)
  OR NOT EXISTS (SELECT 1 FROM job WHERE id = new.job_id AND state = 'working')
  OR EXISTS (SELECT 1 FROM job_session WHERE job_id = new.job_id AND reaped_at IS NULL)
  OR new.helper_pid IS NOT NULL OR new.agent_pgid IS NOT NULL
  OR new.started_at IS NOT NULL OR new.ended_at IS NOT NULL OR new.end_reason IS NOT NULL
  OR new.exit_code IS NOT NULL OR new.reaped_at IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'job: a session starts for a job in working with no live session, with no move done');
END;

-- An update does one of five moves, and nothing else:
--   helper: helper_pid set once, while the session is live;
--   spawn:  agent_pgid set once, after the helper, while the session is live;
--   start:  started_at set once, while the session is open;
--   end:    ended_at and end_reason (and exit_code) set once;
--   reap:   reaped_at set once, after the end.
CREATE TRIGGER job_session_update BEFORE UPDATE ON job_session
WHEN NOT (
  new.id IS old.id AND new.job_id IS old.job_id AND new.agent IS old.agent
  AND new.registered_at IS old.registered_at
  AND (
    (old.helper_pid IS NULL AND new.helper_pid IS NOT NULL AND old.reaped_at IS NULL
      AND new.agent_pgid IS old.agent_pgid AND new.started_at IS old.started_at
      AND new.ended_at IS old.ended_at AND new.end_reason IS old.end_reason
      AND new.exit_code IS old.exit_code AND new.reaped_at IS old.reaped_at)
    OR (old.agent_pgid IS NULL AND new.agent_pgid IS NOT NULL AND old.helper_pid IS NOT NULL
      AND old.reaped_at IS NULL
      AND new.helper_pid IS old.helper_pid AND new.started_at IS old.started_at
      AND new.ended_at IS old.ended_at AND new.end_reason IS old.end_reason
      AND new.exit_code IS old.exit_code AND new.reaped_at IS old.reaped_at)
    OR (old.started_at IS NULL AND new.started_at IS NOT NULL AND old.ended_at IS NULL
      AND new.helper_pid IS old.helper_pid AND new.agent_pgid IS old.agent_pgid
      AND new.ended_at IS old.ended_at AND new.end_reason IS old.end_reason
      AND new.exit_code IS old.exit_code AND new.reaped_at IS old.reaped_at)
    OR (old.ended_at IS NULL AND new.ended_at IS NOT NULL
      AND new.helper_pid IS old.helper_pid AND new.agent_pgid IS old.agent_pgid
      AND new.started_at IS old.started_at AND new.reaped_at IS old.reaped_at)
    OR (old.reaped_at IS NULL AND new.reaped_at IS NOT NULL AND old.ended_at IS NOT NULL
      AND new.helper_pid IS old.helper_pid AND new.agent_pgid IS old.agent_pgid
      AND new.started_at IS old.started_at AND new.ended_at IS old.ended_at
      AND new.end_reason IS old.end_reason AND new.exit_code IS old.exit_code)
  )
)
BEGIN
  SELECT RAISE(ABORT, 'job: a session changes only by one of its five moves');
END;

CREATE TRIGGER job_session_delete BEFORE DELETE ON job_session
BEGIN
  SELECT RAISE(ABORT, 'job: a session row is never deleted');
END;

-- A revoke of a scope row ends each open session of its job, in the same
-- transaction, so the grant of a running agent ends with the revoke.
CREATE TRIGGER job_session_revoke AFTER UPDATE OF revoked_at ON job_scope
WHEN old.revoked_at IS NULL AND new.revoked_at IS NOT NULL
BEGIN
  UPDATE job_session SET ended_at = new.revoked_at, end_reason = 'revoke'
  WHERE job_id = new.job_id AND ended_at IS NULL;
END;

-- A move of the job out of `working` ends each open session of the job, at
-- the time of the move.
CREATE TRIGGER job_session_job_state AFTER UPDATE OF state ON job
WHEN new.state <> 'working'
BEGIN
  UPDATE job_session SET ended_at = new.updated_at, end_reason = 'job_state'
  WHERE job_id = new.id AND ended_at IS NULL;
END;
