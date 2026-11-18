-- Migration 37: the category switch of an Outlook account, and a new
-- understanding after each change of SENT or DRAFT
-- (docs/specs/2026-11-17-outlook-provider.md, assumption A6, card R6-16).
-- The time in milliseconds when Graph accepted a category write but kept
-- another list than the request. From then on, each category write of the
-- account gets a definite refusal. NULL: category writes are on.

ALTER TABLE account ADD COLUMN categories_off_at INTEGER;

-- SENT and DRAFT decide if a message is from you (specification 1, section
-- 6.7), so each change of them needs the understanding again. The triggers
-- hold for each writer of `message_label`: the sync, the apply mirror and
-- the send completion. A writer changes only the rows that differ, so a
-- label that stays fires nothing. An ignored INSERT fires no trigger.
CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id) IN ('SENT', 'DRAFT')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = NEW.message_id AND understood_at IS NOT NULL;
END;

CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id) IN ('SENT', 'DRAFT')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = OLD.message_id AND understood_at IS NOT NULL;
END;
