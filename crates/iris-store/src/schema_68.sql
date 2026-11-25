-- Migration 68 (issue 1049): the understanding of a message records the
-- earlier messages that its run gave the model as context, so that a
-- change of that context runs the message again.
--
-- 1. `message.context`: the row IDs of the context of the last successful
--    run, as text (`iris_understand::store::context_key`): "t:7@..,4@..;s:4@..",
--    each entry a row ID and a hash of the text that a run read of it,
--    so that a new text of the same row changes the record. `t` holds the
--    newest 3 earlier messages of the thread, each with a hash of its
--    entry of the thread context (sender, subject and the first 2,000
--    characters of the clean text). `s` holds the earlier messages of the
--    same sender, each with a hash of its clean text. "t!:" marks a run
--    whose model data held only part of the thread context (the budget
--    cut it, or no model call ran): each entry then also holds the count
--    of the bytes that the data held, and "+" if the text had more bytes.
--    The hash covers only the held bytes. "p:" holds a hash of the known
--    parties of the thread up to the message (limit #1135: a known person
--    only in To or Cc puts no thread in `talk_due`). "-" marks hostile mail: no context shaped it.
--    NULL for a message that no run understood. "?" for an understood
--    message from before this migration (step 3; limit issue #1119: a
--    change before the migration is not seen).
-- 2. A check compares the recorded context of each understood message of
--    a thread with its context now, and clears `understood_at` where they
--    differ (`understand_reset` of migration 65 then clears `tries` and
--    `retry_at`). It runs for each thread in `talk_due`, and for the next
--    few later messages after each run that ends (a success or the last
--    failed try). A difference that is only a message that waits for a run
--    waits for that run, so one change runs a message only once. A message that goes away changes the context of the
--    later messages too, so it puts its thread in `talk_due`
--    (`context_gone`). So does the start and the end of a link check
--    (`link_check`, migration 20): while it waits, a message may be
--    hostile, and it is not context (`may_be_hostile!`). So does a new
--    or changed clean text (`body`), a new or changed sender address
--    (`header`), and a message that sync moves to another thread or
--    another date (both threads).
ALTER TABLE message ADD COLUMN context TEXT;

-- 3. An understood message from before this migration gets "?": its
--    context is not known yet. The queue step records the context of each
--    such message whose thread is not in `talk_due`, before it checks the
--    threads of the queue. A change after this migration puts the thread in
--    `talk_due` first, so the check of a "?" message runs it again: a
--    change after the migration is never lost.
UPDATE message SET context = '?' WHERE understood_at IS NOT NULL;
CREATE INDEX message_context_unknown ON message(id) WHERE context = '?';

CREATE TRIGGER context_gone AFTER UPDATE OF gone_at ON message
  WHEN OLD.gone_at IS NULL AND NEW.gone_at IS NOT NULL AND NEW.thread_id IS NOT NULL
BEGIN
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
END;

CREATE TRIGGER context_link_check_add AFTER INSERT ON link_check
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = NEW.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_link_check_end AFTER DELETE ON link_check
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = OLD.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_body_add AFTER INSERT ON body
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = NEW.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_body AFTER UPDATE ON body
  WHEN OLD.text IS NOT NEW.text OR OLD.sender IS NOT NEW.sender
    OR OLD.subject IS NOT NEW.subject OR OLD.message_id IS NOT NEW.message_id
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id IN (OLD.message_id, NEW.message_id) AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_body_remove AFTER DELETE ON body
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = OLD.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_move AFTER UPDATE OF thread_id, internal_date ON message
  WHEN OLD.thread_id IS NOT NEW.thread_id OR OLD.internal_date IS NOT NEW.internal_date
BEGIN
  INSERT INTO talk_due(thread_id) SELECT OLD.thread_id
    WHERE OLD.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = OLD.thread_id);
  INSERT INTO talk_due(thread_id) SELECT NEW.thread_id
    WHERE NEW.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = NEW.thread_id);
END;

CREATE TRIGGER context_header_add AFTER INSERT ON header
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = NEW.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_header AFTER UPDATE OF from_addr, to_addrs, cc_addrs, message_id ON header
  WHEN OLD.from_addr IS NOT NEW.from_addr OR OLD.to_addrs IS NOT NEW.to_addrs
    OR OLD.cc_addrs IS NOT NEW.cc_addrs OR OLD.message_id IS NOT NEW.message_id
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id IN (OLD.message_id, NEW.message_id) AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

CREATE TRIGGER context_header_remove AFTER DELETE ON header
BEGIN
  INSERT INTO talk_due(thread_id) SELECT m.thread_id FROM message m
    WHERE m.id = OLD.message_id AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due t WHERE t.thread_id = m.thread_id);
END;

-- The known parties of a thread (`known_parties`) show the name of each
-- known person: a new name changes the data of the model. The kind and
-- the address have the triggers of migration 40.
CREATE TRIGGER context_sender_name AFTER UPDATE OF name ON sender
  WHEN OLD.name IS NOT NEW.name AND NEW.kind = 'person'
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM header h JOIN message m ON m.id = h.message_id
    WHERE h.from_addr = lower(NEW.address) AND m.thread_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM talk_due d WHERE d.thread_id = m.thread_id);
END;
