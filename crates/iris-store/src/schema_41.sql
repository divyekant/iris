-- Migration 41: a confirmed sent copy of Iris keeps SENT in the mirror of an
-- Outlook account (docs/specs/2026-11-17-outlook-provider.md, section 6,
-- issue #719).
--
-- Outlook gives SENT only to a copy whose content the send completion
-- confirmed. A sync can read the labels of a copy before the completion
-- commits, and write them after it. The triggers keep SENT for each writer
-- of `message_label`, in the same transaction as the write, so the order of
-- the sync and the completion does not matter.

-- The sent copies that Iris confirmed: `iris_apply::sent_by_iris` reads this
-- view too. A send whose copy had other content (the ledger error ends with
-- `iris_apply::SENT_OTHER_END`) is left out.
CREATE VIEW iris_sent_copy AS
  SELECT d.account_id, d.sent_message AS provider_id FROM draft d
  WHERE d.sent_message IS NOT NULL
    AND NOT EXISTS (SELECT 1 FROM action_draft ad JOIN ledger l ON l.action_id = ad.action_id
                    WHERE ad.draft_id = d.id
                      AND l.error LIKE '% sent other content than the approved content');

-- A write that removes SENT from a confirmed copy does nothing. The delete of
-- the message itself still removes its rows (the message is gone first).
CREATE TRIGGER sent_copy_keep BEFORE DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id) = 'SENT'
   AND EXISTS (SELECT 1 FROM message m JOIN account a ON a.id = m.account_id
               JOIN iris_sent_copy s ON s.account_id = m.account_id AND s.provider_id = m.provider_id
               WHERE m.id = OLD.message_id AND a.provider = 'outlook')
BEGIN
  SELECT RAISE(IGNORE);
END;

-- A label write of a confirmed copy with no SENT adds SENT. The completion
-- of a send adds SENT to a mirrored copy in its own transaction
-- (`iris_apply::send`), and the backfill at the end adds it to each copy
-- that a store confirmed before this migration.
CREATE TRIGGER sent_copy_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id) <> 'SENT'
   AND EXISTS (SELECT 1 FROM message m JOIN account a ON a.id = m.account_id
               JOIN iris_sent_copy s ON s.account_id = m.account_id AND s.provider_id = m.provider_id
               WHERE m.id = NEW.message_id AND a.provider = 'outlook')
BEGIN
  INSERT OR IGNORE INTO message_label(message_id, label_id)
    SELECT NEW.message_id, l.id FROM label l JOIN message m ON m.account_id = l.account_id
    WHERE m.id = NEW.message_id AND l.provider_id = 'SENT';
END;

-- The backfill: each confirmed copy of an Outlook account in the mirror.
INSERT OR IGNORE INTO message_label(message_id, label_id)
  SELECT m.id, l.id FROM iris_sent_copy s
  JOIN account a ON a.id = s.account_id AND a.provider = 'outlook'
  JOIN message m ON m.account_id = s.account_id AND m.provider_id = s.provider_id
  JOIN label l ON l.account_id = s.account_id AND l.provider_id = 'SENT';
