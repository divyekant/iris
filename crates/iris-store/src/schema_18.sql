-- Migration 18: the hostile mark `link` (card R5-10, specification 1,
-- section 6.7).
--
-- One row for each live message whose content has not had the link check.
-- Each batch run first reads all rows (`iris_understand::store::recheck_links`),
-- before it makes the model: the check needs no model, so a model error or a
-- message with failed tries cannot hold it back. A message with a deceptive
-- link is understood at once with no model, so it goes to quarantine. The
-- row goes when the check read the file and the store has its result.
--
-- The rows at the migration: each live message. Mail understood before the
-- mark had no link check.

-- `id` changes each time a trigger queues the message again, so a check
-- that read an older state does not remove the row of a newer one.
CREATE TABLE link_check (
  -- AUTOINCREMENT: a new row never gets the ID of an older one.
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id INTEGER NOT NULL UNIQUE REFERENCES message(id) ON DELETE CASCADE
);

INSERT INTO link_check(message_id)
  SELECT id FROM message WHERE gone_at IS NULL AND content_sha256 IS NOT NULL;

-- New content, and a message that comes back, need the check again.
CREATE TRIGGER link_check_insert AFTER INSERT ON message
  WHEN NEW.content_sha256 IS NOT NULL AND NEW.gone_at IS NULL
BEGIN
  -- A new row, with a new ID. No OR IGNORE or OR REPLACE: the conflict
  -- rule of the outer statement would replace it.
  DELETE FROM link_check WHERE message_id = NEW.id;
  INSERT INTO link_check(message_id) VALUES (NEW.id);
END;

CREATE TRIGGER link_check_update AFTER UPDATE OF content_sha256, gone_at ON message
  WHEN NEW.content_sha256 IS NOT NULL AND NEW.gone_at IS NULL
   AND (OLD.content_sha256 IS NOT NEW.content_sha256 OR OLD.gone_at IS NOT NULL)
BEGIN
  -- A new row, with a new ID. No OR IGNORE or OR REPLACE: the conflict
  -- rule of the outer statement would replace it.
  DELETE FROM link_check WHERE message_id = NEW.id;
  INSERT INTO link_check(message_id) VALUES (NEW.id);
END;

-- The check skips your own mail (`SENT`, `DRAFT`). When such a label goes,
-- the message is received mail, and needs the check.
CREATE TRIGGER link_check_label AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id) IN ('SENT', 'DRAFT')
BEGIN
  DELETE FROM link_check WHERE message_id = OLD.message_id;
  INSERT INTO link_check(message_id) SELECT m.id FROM message m
    WHERE m.id = OLD.message_id AND m.content_sha256 IS NOT NULL AND m.gone_at IS NULL;
END;
