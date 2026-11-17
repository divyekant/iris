-- Migration 19: the scope of a job (card R4-02,
-- docs/specs/2026-11-16-job-workspace.md).
--
-- The threads and the items that the person gives to a job. The workspace
-- of the job holds only the mail of its active scope. Only the person or
-- the engine adds to a scope or revokes a ref: no `agent` value exists, so
-- an agent cannot change the scope of its own job. The only change to a
-- row is one revoke, and a row is never deleted, as the events of a job.
--
-- No foreign key to thread or item: the cleanup of gone mail can delete a
-- row, and the build skips a missing row.

CREATE TABLE job_scope (
  id INTEGER PRIMARY KEY,
  job_id INTEGER NOT NULL REFERENCES job(id),
  thread_id INTEGER CHECK (thread_id IS NULL OR (typeof(thread_id) = 'integer' AND thread_id > 0)),
  item_id INTEGER CHECK (item_id IS NULL OR (typeof(item_id) = 'integer' AND item_id > 0)),
  by TEXT NOT NULL CHECK (by IN ('person', 'engine')),
  at INTEGER NOT NULL CHECK (typeof(at) = 'integer' AND at BETWEEN 0 AND 253402300799999),
  revoked_by TEXT CHECK (revoked_by IS NULL OR revoked_by IN ('person', 'engine')),
  revoked_at INTEGER CHECK (revoked_at IS NULL OR
    (typeof(revoked_at) = 'integer' AND revoked_at BETWEEN 0 AND 253402300799999)),
  CHECK ((thread_id IS NULL) <> (item_id IS NULL)),
  CHECK ((revoked_by IS NULL) = (revoked_at IS NULL))
);

-- NULL values are distinct in a UNIQUE index, so each ref kind has its own.
-- A revoked ref can be added again, so only the active rows are unique.
CREATE UNIQUE INDEX job_scope_thread ON job_scope(job_id, thread_id)
  WHERE thread_id IS NOT NULL AND revoked_at IS NULL;
CREATE UNIQUE INDEX job_scope_item ON job_scope(job_id, item_id)
  WHERE item_id IS NOT NULL AND revoked_at IS NULL;

-- A BEFORE trigger runs before INSERT OR REPLACE deletes a row, and a
-- REPLACE runs no delete trigger. So the trigger refuses the ID of each
-- row and each active duplicate: no insert replaces a scope row.
CREATE TRIGGER job_scope_insert BEFORE INSERT ON job_scope
WHEN EXISTS (SELECT 1 FROM job_scope WHERE id = new.id)
  OR NOT EXISTS (SELECT 1 FROM job WHERE id = new.job_id AND state <> 'done')
  OR new.revoked_at IS NOT NULL OR new.revoked_by IS NOT NULL
  OR NOT (EXISTS (SELECT 1 FROM thread WHERE id = new.thread_id)
          OR EXISTS (SELECT 1 FROM item WHERE id = new.item_id))
  OR EXISTS (SELECT 1 FROM job_scope WHERE job_id = new.job_id AND revoked_at IS NULL
             AND (thread_id = new.thread_id OR item_id = new.item_id))
BEGIN
  SELECT RAISE(ABORT, 'job: the scope grows only for a job that is not done, once for each active ref that exists');
END;

-- The only change is one revoke of an active row.
CREATE TRIGGER job_scope_update BEFORE UPDATE ON job_scope
WHEN old.revoked_at IS NOT NULL OR new.revoked_at IS NULL
  OR new.job_id IS NOT old.job_id OR new.thread_id IS NOT old.thread_id
  OR new.item_id IS NOT old.item_id OR new.by IS NOT old.by OR new.at IS NOT old.at
  OR new.rowid IS NOT old.rowid
BEGIN
  SELECT RAISE(ABORT, 'job: the only change to a scope row is one revoke');
END;

CREATE TRIGGER job_scope_delete BEFORE DELETE ON job_scope
BEGIN
  SELECT RAISE(ABORT, 'job: a scope row is never deleted');
END;

-- The list of a ref: the messages of the thread of a thread ref, or the
-- source messages of the item of an item ref, at each add. A message that a
-- later sync or understanding adds is not in this list.
CREATE TABLE job_scope_message (
  scope_id INTEGER NOT NULL REFERENCES job_scope(id),
  message_id INTEGER NOT NULL CHECK (typeof(message_id) = 'integer' AND message_id > 0),
  PRIMARY KEY (scope_id, message_id)
) WITHOUT ROWID;

-- A list grows only while its ref is active and its job is not done, and
-- only with a message of the thread or a source of the item of the ref.
CREATE TRIGGER job_scope_message_insert BEFORE INSERT ON job_scope_message
WHEN NOT EXISTS (SELECT 1 FROM job_scope s JOIN job j ON j.id = s.job_id
                 WHERE s.id = new.scope_id AND s.revoked_at IS NULL AND j.state <> 'done'
                   AND (EXISTS (SELECT 1 FROM message m
                                WHERE m.id = new.message_id AND m.thread_id = s.thread_id)
                        OR EXISTS (SELECT 1 FROM item_source i
                                   WHERE i.item_id = s.item_id AND i.message_id = new.message_id)))
BEGIN
  SELECT RAISE(ABORT, 'job: a scope message is a message of the thread or a source of the item of an active ref of an open job');
END;

CREATE TRIGGER job_scope_message_update BEFORE UPDATE ON job_scope_message
BEGIN
  SELECT RAISE(ABORT, 'job: a scope message does not change');
END;

CREATE TRIGGER job_scope_message_delete BEFORE DELETE ON job_scope_message
BEGIN
  SELECT RAISE(ABORT, 'job: a scope message is never deleted');
END;
