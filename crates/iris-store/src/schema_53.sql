-- Migration 53 (decision D29, issue 263): no model call for the mail of a
-- declined sender. Understanding runs the fixed steps on such mail (the
-- header, the clean text, the hostile marks, the sender kind), and saves it
-- as understood with no items and no model call. `model_declined` holds
-- each such message. When the decline stops applying (a reopen, a new kind
-- other than `stranger`, a new address, a delete of the row), a trigger
-- makes these messages pending again, so the next run understands them with
-- the model. The trigger covers each writer of the sender row.

CREATE TABLE IF NOT EXISTS model_declined (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);

CREATE TRIGGER IF NOT EXISTS model_declined_end AFTER UPDATE OF address, kind, declined_at ON sender
  WHEN OLD.declined_at IS NOT NULL AND coalesce(OLD.kind, 'stranger') = 'stranger'
   AND NOT (NEW.declined_at IS NOT NULL AND coalesce(NEW.kind, 'stranger') = 'stranger'
            AND NEW.address IS OLD.address)
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM model_declined d
      JOIN header h ON h.message_id = d.message_id JOIN message m ON m.id = d.message_id
     WHERE h.from_addr = lower(OLD.address)
       AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL, waits_model = 0
   WHERE id IN (SELECT d.message_id FROM model_declined d
                  JOIN header h ON h.message_id = d.message_id
                 WHERE h.from_addr = lower(OLD.address));
  DELETE FROM model_declined WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(OLD.address));
END;

CREATE TRIGGER IF NOT EXISTS model_declined_end_remove AFTER DELETE ON sender
  WHEN OLD.declined_at IS NOT NULL AND coalesce(OLD.kind, 'stranger') = 'stranger'
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM model_declined d
      JOIN header h ON h.message_id = d.message_id JOIN message m ON m.id = d.message_id
     WHERE h.from_addr = lower(OLD.address)
       AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL, waits_model = 0
   WHERE id IN (SELECT d.message_id FROM model_declined d
                  JOIN header h ON h.message_id = d.message_id
                 WHERE h.from_addr = lower(OLD.address));
  DELETE FROM model_declined WHERE message_id IN (
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(OLD.address));
END;

-- The start of a decline (a decline, a new address, a new kind
-- `stranger`): the pending mail of the sender can wait for the model (a
-- budget refusal, a retry time, the try limit). The mail of a declined
-- stranger makes no model call, so it waits no more: the next run
-- understands it with the fixed steps.
CREATE TRIGGER IF NOT EXISTS model_declined_start AFTER UPDATE OF address, kind, declined_at ON sender
  WHEN NEW.declined_at IS NOT NULL AND coalesce(NEW.kind, 'stranger') = 'stranger'
   AND NOT (OLD.declined_at IS NOT NULL AND coalesce(OLD.kind, 'stranger') = 'stranger'
            AND OLD.address IS NEW.address)
BEGIN
  UPDATE message SET tries = 0, retry_at = NULL, waits_model = 0
   WHERE understood_at IS NULL
     AND id IN (SELECT message_id FROM header WHERE from_addr = lower(NEW.address));
END;

CREATE TRIGGER IF NOT EXISTS model_declined_start_add AFTER INSERT ON sender
  WHEN NEW.declined_at IS NOT NULL AND coalesce(NEW.kind, 'stranger') = 'stranger'
BEGIN
  UPDATE message SET tries = 0, retry_at = NULL, waits_model = 0
   WHERE understood_at IS NULL
     AND id IN (SELECT message_id FROM header WHERE from_addr = lower(NEW.address));
END;
