-- Migration 41: a new understanding after each change of a label of
-- `iris_store::DRAFT_OR_COPY` (issue 677).
-- Migration 37 asks for a new understanding after each change of SENT or
-- DRAFT. The understanding also gives no item to a message with UNSENT or
-- SENT_COPY. An IMAP client can set the `\Draft` flag in place, and the
-- sync then adds UNSENT to a message that has items. The triggers now hold
-- each label that the understanding reads to decide your own mail, a draft
-- or a copy.

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
