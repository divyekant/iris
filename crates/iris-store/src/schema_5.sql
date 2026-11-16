-- docs/specs/2026-11-16-cost-control.md, "Waits for a model" (R1-28): a
-- message whose model call was refused (no model, or no budget) is not
-- pending until an input of that refusal changes. The triggers clear the
-- mark on each change of the message or the model row, so no write path
-- of the mirror can forget it.

ALTER TABLE message ADD COLUMN waits_model INTEGER NOT NULL DEFAULT 0
  CHECK (waits_model IN (0, 1));

-- New content, a return from gone, a hostile change or a new understanding
-- state of the message.
CREATE TRIGGER waits_model_message AFTER UPDATE OF content_sha256, gone_at, hostile, tries
  ON message WHEN NEW.waits_model = 1
BEGIN
  UPDATE message SET waits_model = 0 WHERE id = NEW.id;
END;

-- A label change (SPAM makes hostile mail, which needs no model).
CREATE TRIGGER waits_model_label_add AFTER INSERT ON message_label
BEGIN
  UPDATE message SET waits_model = 0 WHERE id = NEW.message_id AND waits_model = 1;
END;

CREATE TRIGGER waits_model_label_remove AFTER DELETE ON message_label
BEGIN
  UPDATE message SET waits_model = 0 WHERE id = OLD.message_id AND waits_model = 1;
END;

-- A new model or a changed model row. A budget change clears nothing here:
-- the runner spends and refunds it. The approval of a budget (`iris
-- understand --approve`) clears the marks in its own write.
CREATE TRIGGER waits_model_setting_add AFTER INSERT ON model_setting
BEGIN
  UPDATE message SET waits_model = 0 WHERE waits_model = 1;
END;

CREATE TRIGGER waits_model_setting AFTER UPDATE OF version ON model_setting
  WHEN NEW.version IS NOT OLD.version
BEGIN
  UPDATE message SET waits_model = 0 WHERE waits_model = 1;
END;

-- The wait after a model error, for the current model row (R1-28). A run
-- that stopped on `model`, `unknown` or `error` adds 1 to `retry_failures`
-- and sets `retry_at`; a run with a call and no error sets them back. `iris
-- model set` replaces the row, so a new model has no wait. No trigger reads
-- these columns.
ALTER TABLE model_setting ADD COLUMN retry_failures INTEGER NOT NULL DEFAULT 0
  CHECK (retry_failures >= 0);
ALTER TABLE model_setting ADD COLUMN retry_at INTEGER;
