-- Migration 48: the due date of a job moves it to needs you (decision D24,
-- docs/specs/2026-11-24-job-due-date.md, issue #248).
--
-- The trigger job_event_move of migration 14 again, with one more row of
-- the move table: the engine moves a job in working or waiting to
-- needs_you, with the reason `due`, at or after its due date. The row uses
-- `IS` and `coalesce`, so a NULL reason or a NULL due date makes the row
-- false, not NULL: a NULL would turn the WHEN of the trigger false and
-- allow the move. Each other row of the table stays the same.

DROP TRIGGER job_event_move;

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
          -- due (decision D24): the engine moves a job on its due date
          OR (new.state = 'needs_you' AND new.by = 'engine' AND new.why IS 'due'
              AND coalesce((SELECT due_at FROM job WHERE id = new.job_id) <= new.at, 0)
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
          -- due (decision D24): the engine moves a job on its due date
          OR (new.state = 'needs_you' AND new.by = 'engine' AND new.why IS 'due'
              AND coalesce((SELECT due_at FROM job WHERE id = new.job_id) <= new.at, 0)
              AND new.wait_until IS NULL AND new.answer IS NULL AND new.end_kind IS NULL)
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
