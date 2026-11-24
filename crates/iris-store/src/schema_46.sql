-- Decision D31 (issue 111): after a change of a sender kind to or from
-- `service`, or a change of the name of a known person, the fixed rules
-- that read these facts run again on the affected mail, with no model call.
-- The code-signal rule reads the kind `service`. The spoof mark reads the
-- names of the known persons. Each trigger queues the received mail that a
-- change can affect in `fixed_due`. The understanding runner reads the
-- queue first (`iris_understand::store::recheck_fixed`), and understands a
-- message again with no model only when its fixed result changed.
--
-- A trigger deletes the old row of a message and inserts a new one (a new
-- `id`), with no conflict clause: an outer upsert replaces the conflict
-- clause of a trigger. A check removes only the row that it read, so a
-- change during a check queues the message again. The triggers cover each
-- writer of the sender row: the sender axis, the stranger gate and a direct
-- SQL write. A row with no kind is a stranger, as no row is.
-- shortcut: the name triggers read `header` with no index on `from_name`;
-- add an index if a large mailbox makes a name change slow.

CREATE TABLE IF NOT EXISTS fixed_due (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS fixed_due_message ON fixed_due(message_id);

-- The mail from the address of a row whose kind becomes or stops being
-- `service`, or whose address changes.
CREATE TRIGGER IF NOT EXISTS fixed_due_service AFTER UPDATE OF address, kind ON sender
  WHEN (OLD.kind IS 'service') IS NOT (NEW.kind IS 'service')
    OR (OLD.address IS NOT NEW.address AND 'service' IN (OLD.kind, NEW.kind))
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_addr IN (lower(OLD.address), lower(NEW.address)));
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE h.from_addr IN (lower(OLD.address), lower(NEW.address)) AND m.gone_at IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS fixed_due_service_add AFTER INSERT ON sender
  WHEN NEW.kind IS 'service'
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(NEW.address));
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE h.from_addr = lower(NEW.address) AND m.gone_at IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS fixed_due_service_remove AFTER DELETE ON sender
  WHEN OLD.kind IS 'service'
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(OLD.address));
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE h.from_addr = lower(OLD.address) AND m.gone_at IS NULL;
END;

-- The mail under the old and the new name of a known person: the name of
-- a row of the kind `person` comes, goes or changes. Names compare as the
-- spoof check compares them (NOCASE).
CREATE TRIGGER IF NOT EXISTS fixed_due_name AFTER UPDATE OF kind, name ON sender
  WHEN (CASE WHEN OLD.kind IS 'person' THEN lower(OLD.name) END)
    IS NOT (CASE WHEN NEW.kind IS 'person' THEN lower(NEW.name) END)
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h
     WHERE (OLD.kind IS 'person' AND h.from_name = OLD.name COLLATE NOCASE)
        OR (NEW.kind IS 'person' AND h.from_name = NEW.name COLLATE NOCASE));
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE m.gone_at IS NULL
       AND ((OLD.kind IS 'person' AND h.from_name = OLD.name COLLATE NOCASE)
         OR (NEW.kind IS 'person' AND h.from_name = NEW.name COLLATE NOCASE));
END;

CREATE TRIGGER IF NOT EXISTS fixed_due_name_add AFTER INSERT ON sender
  WHEN NEW.kind IS 'person' AND NEW.name IS NOT NULL
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_name = NEW.name COLLATE NOCASE);
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE m.gone_at IS NULL AND h.from_name = NEW.name COLLATE NOCASE;
END;

CREATE TRIGGER IF NOT EXISTS fixed_due_name_remove AFTER DELETE ON sender
  WHEN OLD.kind IS 'person' AND OLD.name IS NOT NULL
BEGIN
  DELETE FROM fixed_due WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_name = OLD.name COLLATE NOCASE);
  INSERT INTO fixed_due(message_id)
    SELECT h.message_id FROM header h JOIN message m ON m.id = h.message_id
     WHERE m.gone_at IS NULL AND h.from_name = OLD.name COLLATE NOCASE;
END;
