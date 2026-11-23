-- Migration 41: a message with a label of `iris_store::DRAFT_OR_COPY` keeps
-- no item of its own understanding (issue 677).
-- An IMAP client can set the `\Draft` flag in place, and the sync then adds
-- UNSENT to a message that has items. The batch runner skips each message
-- with such a label, so no understanding runs to drop them. The rule now
-- holds at the label write, for each writer of `message_label` (the sync,
-- the apply mirror and the send completion).

-- The items that a new understanding of a message replaces: origin items
-- of a message that is not gone, made by a rule or the model, never a
-- conversation and never a pinned item. An item whose origin message is
-- gone counts as pinned (spec 1, section 7.5). `drop_old_items` and `rerun_senders` of `iris-understand` read this
-- view too, so the three paths share one rule.
CREATE VIEW item_droppable AS
  SELECT src.message_id AS message_id, i.id AS item_id
  FROM item_source src JOIN item i ON i.id = src.item_id
  JOIN message m ON m.id = src.message_id
  WHERE src.role = 'origin' AND m.gone_at IS NULL AND i.form <> 'conversation'
    AND i.derived_by IN ('rule', 'model')
    AND NOT EXISTS (SELECT 1 FROM item_pin p WHERE p.item_id = i.id);

-- The change of each label that decides own mail, a draft or a copy clears
-- `understood_at`, as migration 37 does for SENT and DRAFT.
DROP TRIGGER understand_owner_label_add;
DROP TRIGGER understand_owner_label_remove;

CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = NEW.message_id AND understood_at IS NOT NULL;
END;

CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = OLD.message_id AND understood_at IS NOT NULL;
END;

-- A new DRAFT, UNSENT or SENT_COPY label deletes the droppable items of the
-- message.
CREATE TRIGGER draft_or_copy_drops_items AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id)
    IN ('DRAFT', 'UNSENT', 'SENT_COPY')
BEGIN
  DELETE FROM item WHERE id IN
    (SELECT item_id FROM item_droppable WHERE message_id = NEW.message_id);
END;

-- The conversation of a thread leaves out drafts and copies, so each new or
-- removed DRAFT, UNSENT or SENT_COPY label puts the thread in `talk_due`
-- (migration 40), also when the message had no droppable item.
CREATE TRIGGER draft_or_copy_talk_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id)
    IN ('DRAFT', 'UNSENT', 'SENT_COPY')
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = NEW.message_id
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER draft_or_copy_talk_remove AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id)
    IN ('DRAFT', 'UNSENT', 'SENT_COPY')
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = OLD.message_id
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

-- A store before this migration can hold such items, and conversations
-- that show such a message. Each thread with a draft or a copy is computed
-- again once.
INSERT INTO talk_due(thread_id) SELECT DISTINCT m.thread_id FROM message m
  WHERE EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
      WHERE ml.message_id = m.id AND l.provider_id IN ('DRAFT', 'UNSENT', 'SENT_COPY'))
    AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
DELETE FROM item WHERE id IN (
  SELECT d.item_id FROM item_droppable d
  WHERE EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
    WHERE ml.message_id = d.message_id
      AND l.provider_id IN ('DRAFT', 'UNSENT', 'SENT_COPY')));
