-- Migration 64 (issue 110): a change that makes a message need the
-- understanding again also gives it a new try budget, and the label SPAM
-- is such a change.
--
-- Invariant: after each such change, `tries` is 0 and `retry_at` is NULL,
-- also for a message that failed the model before the change (its
-- `understood_at` is NULL already).
--
-- 1. SPAM decides if a message is hostile mail (specification 1, section
--    6.7). Understanding drops the items of hostile mail, so a message that
--    leaves SPAM (a false positive) needs the understanding again to get its
--    items. A message that gets SPAM drops its items at the next run; the
--    reads hide them before that (`hostile_mail!`). The label triggers of
--    migration 42 get SPAM, and clear `tries` and `retry_at` too.
--    When SPAM comes, the droppable items of the message go at once (as
--    for DRAFT, UNSENT and SENT_COPY in migration 42), so an old item does
--    not show again when SPAM goes before the next run (trigger
--    `spam_drops_items`). A pinned item stays. The same holds for a gone
--    message that comes back with SPAM (`spam_back_drops_items`), and for
--    each message that has SPAM at this migration (the DELETE below).
--    Invariant: a message that is not gone and has SPAM has no droppable
--    item.
-- 2. A gone message that comes back, and new content, clear
--    `understood_at`, `tries` and `retry_at` for each writer (triggers
--    `understand_come_back` and `understand_new_content`). The content
--    cleanup sets `content_sha256` to NULL on gone mail; that is not new
--    content.
--    Each reset trigger also puts the thread in `talk_due`, also for a
--    message whose `understood_at` is NULL already.
-- 3. Each reset of an understood message (`understood_at` goes to NULL, and
--    `tries` does not grow) clears `tries` and `retry_at` and puts its thread
--    in `talk_due`, so the conversation is computed again. This covers each
--    other writer too (for example a new hostile mark). A failed try adds 1
--    to `tries`, so the condition `NEW.tries <= OLD.tries` keeps it out.

DROP TRIGGER IF EXISTS understand_owner_label_add;
DROP TRIGGER IF EXISTS understand_owner_label_remove;

CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY', 'SPAM')
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = NEW.message_id AND m.thread_id IS NOT NULL
      AND (m.understood_at IS NOT NULL OR m.tries <> 0 OR m.retry_at IS NOT NULL)
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL
    WHERE id = NEW.message_id
      AND (understood_at IS NOT NULL OR tries <> 0 OR retry_at IS NOT NULL);
END;

CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY', 'SPAM')
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = OLD.message_id AND m.thread_id IS NOT NULL
      AND (m.understood_at IS NOT NULL OR m.tries <> 0 OR m.retry_at IS NOT NULL)
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL
    WHERE id = OLD.message_id
      AND (understood_at IS NOT NULL OR tries <> 0 OR retry_at IS NOT NULL);
END;

DROP TRIGGER IF EXISTS understand_reset;

CREATE TRIGGER understand_reset AFTER UPDATE OF understood_at ON message
  WHEN OLD.understood_at IS NOT NULL AND NEW.understood_at IS NULL
   AND NEW.tries <= OLD.tries
BEGIN
  UPDATE message SET tries = 0, retry_at = NULL WHERE id = NEW.id;
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NEW.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
END;

DROP TRIGGER IF EXISTS understand_new_content;

CREATE TRIGGER understand_new_content AFTER UPDATE OF content_sha256 ON message
  WHEN OLD.content_sha256 IS NOT NEW.content_sha256 AND NEW.content_sha256 IS NOT NULL
   AND (NEW.understood_at IS NOT NULL OR NEW.tries <> 0 OR NEW.retry_at IS NOT NULL)
BEGIN
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NEW.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL WHERE id = NEW.id;
END;

DROP TRIGGER IF EXISTS understand_come_back;

CREATE TRIGGER understand_come_back AFTER UPDATE OF gone_at ON message
  WHEN OLD.gone_at IS NOT NULL AND NEW.gone_at IS NULL
   AND (NEW.understood_at IS NOT NULL OR NEW.tries <> 0 OR NEW.retry_at IS NOT NULL)
BEGIN
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NEW.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
  UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL WHERE id = NEW.id;
END;

DROP TRIGGER IF EXISTS spam_drops_items;

CREATE TRIGGER spam_drops_items AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id) = 'SPAM'
BEGIN
  DELETE FROM item WHERE id IN
    (SELECT item_id FROM item_droppable WHERE message_id = NEW.message_id);
END;

DROP TRIGGER IF EXISTS spam_back_drops_items;

CREATE TRIGGER spam_back_drops_items AFTER UPDATE OF gone_at ON message
  WHEN OLD.gone_at IS NOT NULL AND NEW.gone_at IS NULL
  AND EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
    WHERE ml.message_id = NEW.id AND l.provider_id = 'SPAM')
BEGIN
  DELETE FROM item WHERE id IN
    (SELECT item_id FROM item_droppable WHERE message_id = NEW.id);
END;

-- The live messages that have SPAM now can hold an understanding from
-- before the label. Each one gets the understanding again (SPAM makes no
-- model call), and its thread is computed again. Their droppable items go.
INSERT OR IGNORE INTO talk_due(thread_id)
  SELECT DISTINCT m.thread_id FROM message m
    JOIN message_label ml ON ml.message_id = m.id
    JOIN label l ON l.id = ml.label_id
  WHERE l.provider_id = 'SPAM' AND m.thread_id IS NOT NULL AND m.gone_at IS NULL;
UPDATE message SET understood_at = NULL, tries = 0, retry_at = NULL
  WHERE gone_at IS NULL
    AND (understood_at IS NOT NULL OR tries <> 0 OR retry_at IS NOT NULL)
    AND EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
      WHERE ml.message_id = message.id AND l.provider_id = 'SPAM');
INSERT OR IGNORE INTO talk_due(thread_id)
  SELECT DISTINCT m.thread_id FROM item_droppable d
    JOIN message m ON m.id = d.message_id
    JOIN message_label ml ON ml.message_id = d.message_id
    JOIN label l ON l.id = ml.label_id
  WHERE l.provider_id = 'SPAM' AND m.thread_id IS NOT NULL;
DELETE FROM item WHERE id IN
  (SELECT d.item_id FROM item_droppable d
     JOIN message_label ml ON ml.message_id = d.message_id
     JOIN label l ON l.id = ml.label_id
   WHERE l.provider_id = 'SPAM');
