-- Migration 18: drafts and sends (docs/specs/2026-11-16-send.md, card R3-01).
--
-- 1. The verbs `draft` and `send` join the verb CHECK of changeset_action.
--    SQLite cannot change a CHECK in place. A wider CHECK accepts each
--    stored row, so the migration changes only the text of the table in
--    sqlite_schema (the SQLite procedure for a schema change that does not
--    change stored rows), and RESET loads the schema again. The CREATE
--    TABLE statements below change the schema cookie, so each other
--    connection loads the new schema too.
-- 2. The table `draft`: one row for each draft that Iris made at Gmail.
-- 3. The table `action_draft`: the draft target of an action, from submit.
-- 4. The table `ledger_send`: the one send of each draft that can have
--    gone out.

PRAGMA writable_schema = ON;
UPDATE sqlite_schema
SET sql = replace(sql, '''decline'', ''correct''))', '''decline'', ''correct'', ''draft'', ''send''))')
WHERE type = 'table' AND name = 'changeset_action';
PRAGMA writable_schema = RESET;

-- Stop the migration if the text above did not change: the CHECK of a
-- temporary table refuses 0.
CREATE TEMP TABLE migration_18 (ok INTEGER CHECK (ok = 1));
INSERT INTO migration_18
SELECT instr(sql, '''correct'', ''draft'', ''send'')') > 0
FROM sqlite_schema WHERE type = 'table' AND name = 'changeset_action';
DROP TABLE migration_18;

CREATE TABLE draft (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id INTEGER NOT NULL REFERENCES account(id),
  -- The thread of a reply. NULL for a new mail.
  thread_id INTEGER REFERENCES thread(id),
  -- The Gmail draft ID.
  provider_id TEXT NOT NULL,
  -- The Message-ID that Iris set, with no angle brackets. It stays the same
  -- for each change of the draft.
  message_id_hdr TEXT NOT NULL UNIQUE,
  -- The checked content (iris-compose), as JSON, with the reply headers.
  content TEXT NOT NULL CHECK (json_valid(content)),
  -- The content hash (iris_compose::hash) that a send approval binds to.
  hash TEXT NOT NULL,
  -- The exact message that Iris wrote. The release sends these bytes.
  raw BLOB NOT NULL,
  -- The `draft` action that made the draft.
  action_id INTEGER NOT NULL UNIQUE REFERENCES changeset_action(id),
  -- The time of the send that Gmail confirmed, in milliseconds, and the
  -- Gmail ID of the sent message.
  sent_at INTEGER,
  sent_message TEXT,
  -- The time when the engine found a form at Gmail that Iris does not
  -- write (for example an attachment or a Bcc). Iris then no longer
  -- changes or sends the draft.
  foreign_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK ((sent_at IS NULL) = (sent_message IS NULL)),
  UNIQUE (account_id, provider_id)
);

-- The draft of a `send` action. Submit writes it from the targets
-- `{"drafts": ["d-N"]}`.
CREATE TABLE action_draft (
  action_id INTEGER PRIMARY KEY REFERENCES changeset_action(id),
  draft_id INTEGER NOT NULL REFERENCES draft(id)
);

CREATE INDEX action_draft_draft ON action_draft(draft_id);

-- The draft of a submitted action does not change, as the messages and
-- the items of an action (migrations 7 and 10): the approval binds to it.
CREATE TRIGGER action_draft_insert BEFORE INSERT ON action_draft
WHEN (SELECT state FROM changeset_action WHERE id = new.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

CREATE TRIGGER action_draft_delete BEFORE DELETE ON action_draft
WHEN (SELECT state FROM changeset_action WHERE id = old.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- Submit inserts and deletes these rows. Nothing updates them.
CREATE TRIGGER action_draft_update BEFORE UPDATE ON action_draft
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- The one send of each draft that can have gone out. The claim of a send
-- inserts the row; the primary key allows one claim for each draft, for
-- each writer. Only a definite rejection of the send deletes the row, in
-- the same transaction as its `failed` state. An unknown result keeps it
-- (issue #280).
CREATE TABLE ledger_send (
  draft_id INTEGER PRIMARY KEY REFERENCES draft(id),
  ledger_id INTEGER NOT NULL UNIQUE REFERENCES ledger(id)
);
