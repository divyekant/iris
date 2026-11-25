-- Migration 59 (issue 110): a change that makes a message need the
-- understanding again also gives it a new try budget, and the label SPAM
-- is such a change.
--
-- 1. SPAM decides if a message is hostile mail (specification 1, section
--    6.7). Understanding drops the items of hostile mail, so a message that
--    leaves SPAM (a false positive) needs the understanding again to get its
--    items. A message that gets SPAM drops its items at the next run; the
--    reads hide them before that (`hostile_mail!`). The label triggers of
--    migration 42 get SPAM.
-- 2. Each reset of `understood_at` (new content, a gone message that comes
--    back, a label of the list above, a new hostile mark) clears `tries` and
--    `retry_at`, so a message that failed before a change is pending again,
--    and puts its thread in `talk_due`, so the conversation is computed
--    again. A failed try also sets `understood_at` to NULL, but it adds 1 to
--    `tries`: the condition `NEW.tries = OLD.tries` keeps that path out.
-- 3. New content also clears `tries` and `retry_at` of a message that was
--    never understood.

DROP TRIGGER IF EXISTS understand_owner_label_add;
DROP TRIGGER IF EXISTS understand_owner_label_remove;

CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY', 'SPAM')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = NEW.message_id AND understood_at IS NOT NULL;
END;

CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label
  WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id)
    IN ('SENT', 'DRAFT', 'UNSENT', 'SENT_COPY', 'SPAM')
BEGIN
  UPDATE message SET understood_at = NULL
    WHERE id = OLD.message_id AND understood_at IS NOT NULL;
END;

DROP TRIGGER IF EXISTS understand_reset;

CREATE TRIGGER understand_reset AFTER UPDATE OF understood_at ON message
  WHEN OLD.understood_at IS NOT NULL AND NEW.understood_at IS NULL
   AND NEW.tries = OLD.tries
BEGIN
  UPDATE message SET tries = 0, retry_at = NULL WHERE id = NEW.id;
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NEW.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
END;

DROP TRIGGER IF EXISTS understand_new_content;

CREATE TRIGGER understand_new_content AFTER UPDATE OF content_sha256 ON message
  WHEN OLD.content_sha256 IS NOT NEW.content_sha256
   AND (NEW.tries <> 0 OR NEW.retry_at IS NOT NULL)
BEGIN
  UPDATE message SET tries = 0, retry_at = NULL WHERE id = NEW.id;
END;
