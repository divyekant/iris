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

CREATE TABLE link_check (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);

INSERT INTO link_check(message_id)
  SELECT id FROM message WHERE gone_at IS NULL AND content_sha256 IS NOT NULL;

-- New content, and a message that comes back, need the check again.
CREATE TRIGGER link_check_insert AFTER INSERT ON message
  WHEN NEW.content_sha256 IS NOT NULL AND NEW.gone_at IS NULL
BEGIN
  -- No OR IGNORE: the conflict rule of the outer statement would replace it.
  INSERT INTO link_check(message_id) SELECT NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM link_check WHERE message_id = NEW.id);
END;

CREATE TRIGGER link_check_update AFTER UPDATE OF content_sha256, gone_at ON message
  WHEN NEW.content_sha256 IS NOT NULL AND NEW.gone_at IS NULL
   AND (OLD.content_sha256 IS NOT NEW.content_sha256 OR OLD.gone_at IS NOT NULL)
BEGIN
  -- No OR IGNORE: the conflict rule of the outer statement would replace it.
  INSERT INTO link_check(message_id) SELECT NEW.id
    WHERE NOT EXISTS (SELECT 1 FROM link_check WHERE message_id = NEW.id);
END;
