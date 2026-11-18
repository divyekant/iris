-- Migration 32: review comments (docs/specs/2026-11-17-review-comments.md,
-- card R4-08). One row for each comment of the person on a revision of a
-- changeset of a job, and the delivery of the comments to the sessions of
-- the job. Nothing changes or removes a row.

CREATE TABLE changeset_comment (
  -- AUTOINCREMENT: the engine never uses an ID again.
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  changeset_id INTEGER NOT NULL REFERENCES changeset(id),
  revision INTEGER NOT NULL CHECK (revision >= 1),
  text TEXT NOT NULL CHECK (
    length(trim(text)) > 0 AND length(text) <= 4000 AND instr(text, char(0)) = 0
  ),
  -- The client makes the key, so that a retry after a lost answer writes
  -- no second row.
  key TEXT NOT NULL CHECK (
    length(key) BETWEEN 16 AND 64 AND key NOT GLOB '*[^A-Za-z0-9_-]*'
  ),
  at_ms INTEGER NOT NULL,
  UNIQUE (changeset_id, key)
);

CREATE TRIGGER changeset_comment_keep BEFORE UPDATE ON changeset_comment
BEGIN
  SELECT RAISE(ABORT, 'a comment never changes');
END;

CREATE TRIGGER changeset_comment_stay BEFORE DELETE ON changeset_comment
BEGIN
  SELECT RAISE(ABORT, 'a comment is never removed');
END;

-- The highest comment ID that the build of a started session read. The
-- launch writes the row in its gate, the transaction that sets started_at.
-- A comment with a higher ID than each row of its job is pending.
CREATE TABLE comment_delivery (
  session_id INTEGER PRIMARY KEY REFERENCES job_session(id),
  comment_id INTEGER NOT NULL REFERENCES changeset_comment(id)
);

CREATE TRIGGER comment_delivery_keep BEFORE UPDATE ON comment_delivery
BEGIN
  SELECT RAISE(ABORT, 'a delivery never changes');
END;

CREATE TRIGGER comment_delivery_stay BEFORE DELETE ON comment_delivery
BEGIN
  SELECT RAISE(ABORT, 'a delivery is never removed');
END;
