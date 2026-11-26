-- Migration 71 (issue 1135): a change of a known person queues the threads
-- where the address is in `To` or `Cc`, so the context check of migration
-- 69 sees each known party of the data of the model.
--
-- The triggers of migrations 40 and 69 queue only the threads where the
-- address is the sender (`From`). A scan of the address lists of each
-- header for each changed sender is slow when a sender refresh changes
-- many senders at once. So the triggers only put the address in
-- `party_due`. The understanding step
-- (`iris_understand::store::queue_party_threads`) reads the headers once
-- for all addresses in the queue, puts each thread with such an address in
-- `To` or `Cc` in `talk_due`, and clears the queue. The triggers use
-- `WHERE NOT EXISTS`, not `OR IGNORE`: the conflict clause of an outer
-- upsert of `sender` replaces the one of a trigger.
CREATE TABLE IF NOT EXISTS party_due (address TEXT PRIMARY KEY);

CREATE TRIGGER IF NOT EXISTS party_due_sender_add AFTER INSERT ON sender
  WHEN NEW.kind = 'person'
BEGIN
  INSERT INTO party_due(address) SELECT lower(NEW.address)
    WHERE NOT EXISTS (SELECT 1 FROM party_due d WHERE d.address = lower(NEW.address));
END;

CREATE TRIGGER IF NOT EXISTS party_due_sender AFTER UPDATE OF address, kind, name ON sender
  WHEN (OLD.kind = 'person' OR NEW.kind = 'person')
    AND (OLD.address IS NOT NEW.address OR OLD.kind IS NOT NEW.kind
         OR OLD.name IS NOT NEW.name)
BEGIN
  INSERT INTO party_due(address) SELECT lower(OLD.address)
    WHERE NOT EXISTS (SELECT 1 FROM party_due d WHERE d.address = lower(OLD.address));
  INSERT INTO party_due(address) SELECT lower(NEW.address)
    WHERE NOT EXISTS (SELECT 1 FROM party_due d WHERE d.address = lower(NEW.address));
END;

CREATE TRIGGER IF NOT EXISTS party_due_sender_remove AFTER DELETE ON sender
  WHEN OLD.kind = 'person'
BEGIN
  INSERT INTO party_due(address) SELECT lower(OLD.address)
    WHERE NOT EXISTS (SELECT 1 FROM party_due d WHERE d.address = lower(OLD.address));
END;

-- A known person who changed before this migration: each one goes in the
-- queue once, so the first pass checks each thread with a known person in
-- `To` or `Cc`. The check runs a message again only where its recorded
-- context differs.
INSERT INTO party_due(address)
  SELECT DISTINCT lower(address) FROM sender WHERE kind = 'person'
     AND lower(address) NOT IN (SELECT address FROM party_due);
