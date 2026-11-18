-- Migration 35: the memory of a job between sessions
-- (docs/specs/2026-11-17-job-memory.md, card R5-06). One copy of
-- `notes.md` of each job, as the bytes of the file. The backup copies
-- `iris.db`, so it holds the copy; it does not copy the workspaces.

CREATE TABLE job_notes (
  job_id   INTEGER PRIMARY KEY REFERENCES job(id),
  text     BLOB NOT NULL CHECK (typeof(text) = 'blob' AND length(text) <= 65536),
  saved_ms INTEGER NOT NULL
);

-- The copy ends with the job, in the transaction of the move to done.
-- Each path to done (the finish of the agent, the finish and the cancel of
-- the person) inserts a job_event with the state done.
CREATE TRIGGER job_notes_end AFTER INSERT ON job_event
WHEN NEW.state = 'done'
BEGIN
  DELETE FROM job_notes WHERE job_id = NEW.job_id;
END;

-- A done job gets no copy. SQLite runs this trigger on each upsert, also on
-- one that takes the UPDATE path, and the move to done cannot go back.
CREATE TRIGGER job_notes_done BEFORE INSERT ON job_notes
WHEN (SELECT state FROM job WHERE id = NEW.job_id) = 'done'
BEGIN
  SELECT RAISE(IGNORE);
END;
