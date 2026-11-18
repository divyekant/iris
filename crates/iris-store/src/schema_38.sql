-- Migration 38: a write of the same values clears no `waits_model` mark
-- (issue #154). A full resync and `Store::put_messages` write the content
-- hash and the gone time of each listed message again, also when they do
-- not change. Before, each such write cleared the mark, so the next run
-- read the waiting mail one more time.

DROP TRIGGER waits_model_message;

CREATE TRIGGER waits_model_message AFTER UPDATE OF content_sha256, gone_at, hostile, tries
  ON message WHEN NEW.waits_model <> 0 AND (
    NEW.content_sha256 IS NOT OLD.content_sha256 OR NEW.gone_at IS NOT OLD.gone_at
    OR NEW.hostile IS NOT OLD.hostile OR NEW.tries IS NOT OLD.tries)
BEGIN
  UPDATE message SET waits_model = 0 WHERE id = NEW.id;
END;
