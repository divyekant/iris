-- Migration 63 (issues 369, 373 and 374): the person can release one
-- message from quarantine. `message.released` holds the hostile marks that
-- the person released (a JSON array of `dmarc`, `spoof` and `link`), for
-- the current content of the message. NULL: no release.
--
-- The understanding step leaves a released mark out of the marks that it
-- computes for the message, so each gate that reads `message.hostile` sees
-- the message as the person decided. The marks `spam` and `phishing` and a
-- `hostile` sender are not released here.
ALTER TABLE message ADD COLUMN released TEXT;

-- A release or the end of a release queues the fixed check
-- (`iris_understand::store::recheck_fixed`), which understands the message
-- again with no model when its marks change. The trigger deletes the old
-- row and inserts a new one, as the triggers of migration 54 do.
CREATE TRIGGER IF NOT EXISTS fixed_due_release AFTER UPDATE OF released ON message
  WHEN OLD.released IS NOT NEW.released AND NEW.gone_at IS NULL
BEGIN
  DELETE FROM fixed_due WHERE message_id = NEW.id;
  INSERT INTO fixed_due(message_id) VALUES (NEW.id);
END;

-- A release is for one content: new content of the message ends it.
CREATE TRIGGER IF NOT EXISTS message_release_content AFTER UPDATE OF content_sha256 ON message
  WHEN OLD.content_sha256 IS NOT NEW.content_sha256 AND NEW.released IS NOT NULL
BEGIN
  UPDATE message SET released = NULL WHERE id = NEW.id;
END;
