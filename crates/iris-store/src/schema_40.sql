-- Issue 106: the conversation of a thread reads the kind of the sender of
-- each message (spec 1 section 6.7). Each change of a sender kind (an
-- insert, an update of `address` or `kind`, a delete) marks each thread
-- with mail from that address. `iris_talk::update_due` computes their
-- conversations again and empties the queue. The triggers use NOT EXISTS,
-- not OR IGNORE: an outer upsert replaces the conflict clause of a trigger. The triggers cover each
-- writer: the sender axis, the stranger gate and a direct SQL write. A row
-- with no kind counts as a stranger, as no row does, so an insert or a
-- delete of such a row (a party row of `iris-talk`) marks nothing.

CREATE TABLE talk_due (thread_id INTEGER PRIMARY KEY);

-- An older store can hold a conversation that a past kind change left
-- stale, with no queue to mark it. Each thread with understood mail is
-- computed again once.
INSERT INTO talk_due(thread_id)
  SELECT DISTINCT thread_id FROM message WHERE understood_at IS NOT NULL;

CREATE TRIGGER talk_due_sender_add AFTER INSERT ON sender
  WHEN NEW.kind IS NOT NULL
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM header h JOIN message m ON m.id = h.message_id
    WHERE h.from_addr = lower(NEW.address)
      AND NOT EXISTS (SELECT 1 FROM talk_due d WHERE d.thread_id = m.thread_id);
END;

CREATE TRIGGER talk_due_sender AFTER UPDATE OF address, kind ON sender
  WHEN OLD.address IS NOT NEW.address OR OLD.kind IS NOT NEW.kind
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM header h JOIN message m ON m.id = h.message_id
    WHERE h.from_addr IN (lower(OLD.address), lower(NEW.address))
      AND NOT EXISTS (SELECT 1 FROM talk_due d WHERE d.thread_id = m.thread_id);
END;

CREATE TRIGGER talk_due_sender_remove AFTER DELETE ON sender
  WHEN OLD.kind IS NOT NULL
BEGIN
  INSERT INTO talk_due(thread_id)
    SELECT DISTINCT m.thread_id FROM header h JOIN message m ON m.id = h.message_id
    WHERE h.from_addr = lower(OLD.address)
      AND NOT EXISTS (SELECT 1 FROM talk_due d WHERE d.thread_id = m.thread_id);
END;
