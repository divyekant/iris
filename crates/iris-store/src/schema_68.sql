-- Migration 68 (issue 1049): the understanding of a message records the
-- earlier messages that its run gave the model as context, so that a
-- change of that context runs the message again.
--
-- 1. `message.context`: the row IDs of the context of the last successful
--    run, as text (`iris_understand::store::context_key`): "t:7,4;s:4",
--    with `t` the newest 3 earlier messages of the thread and `s` the
--    earlier messages of the same sender. "t!:" marks a run whose model
--    data held only the listed thread messages (the budget cut the others,
--    or no model call ran). NULL for a message that no run understood
--    since this migration. The first check of such a row records its
--    context now and runs nothing again (limit issue #1119: a change
--    before the migration is not seen).
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
--    hostile, and it is not context (`may_be_hostile!`).
ALTER TABLE message ADD COLUMN context TEXT;

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
