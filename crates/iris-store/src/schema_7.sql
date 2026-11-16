-- Spec 2 section 6.4: the attention of each item follows the facts of its
-- origin message. Each change of such a fact marks the message here, and
-- the engine evaluates the items of each marked message again
-- (`iris_policy::refresh_attention`). The triggers mark on each write path,
-- so no path of sync, understanding or the sender axis can forget it.
CREATE TABLE attention_due (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);

-- The time of the last evaluation of each item (seconds since 1970-01-01
-- UTC). A rule with `older_than_days` matches more items as time goes on,
-- with no change in the store, so the engine evaluates each item again each
-- hour. No row: each item is due, as after this migration.
CREATE TABLE attention_pass (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  at INTEGER NOT NULL
);

-- A new item of the message (an origin source).
CREATE TRIGGER attention_due_source AFTER INSERT ON item_source
  WHEN NEW.role = 'origin'
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) VALUES (NEW.message_id);
END;

-- A new origin message of an item. The attention of an item depends on
-- the item and its origin message only, so the other items of a message
-- need no mark.
CREATE TRIGGER attention_due_source_move AFTER UPDATE OF message_id, role ON item_source
  WHEN NEW.role = 'origin'
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) VALUES (NEW.message_id);
END;

-- A new form, record type or commitment kind, or a new document with no
-- attention. The attention write itself changes none of them, so it marks
-- nothing.
CREATE TRIGGER attention_due_item AFTER UPDATE OF form, doc ON item
  WHEN OLD.form IS NOT NEW.form
    OR json_extract(OLD.doc, '$.type') IS NOT json_extract(NEW.doc, '$.type')
    OR json_extract(OLD.doc, '$.kind') IS NOT json_extract(NEW.doc, '$.kind')
    OR json_extract(NEW.doc, '$.attention') IS NULL
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id)
    SELECT message_id FROM item_source WHERE item_id = NEW.id AND role = 'origin';
END;

-- A new document with no attention keeps the old attention until the
-- engine evaluates the item again, so a hidden item does not come back to
-- the brief for that time.
CREATE TRIGGER attention_keep AFTER UPDATE OF doc ON item
  WHEN json_extract(NEW.doc, '$.attention') IS NULL
    AND json_extract(OLD.doc, '$.attention') IS NOT NULL
BEGIN
  UPDATE item SET doc = json_set(doc, '$.attention', json(json_extract(OLD.doc, '$.attention')))
    WHERE id = NEW.id;
END;

-- A label, the hostile marks or the date of the message.
CREATE TRIGGER attention_due_label_add AFTER INSERT ON message_label
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT NEW.message_id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = NEW.message_id AND role = 'origin');
END;

CREATE TRIGGER attention_due_label_remove AFTER DELETE ON message_label
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT OLD.message_id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = OLD.message_id AND role = 'origin');
END;

-- A new name of a label: rules match label names.
CREATE TRIGGER attention_due_label_name AFTER UPDATE OF name, provider_id ON label
  WHEN OLD.name IS NOT NEW.name OR OLD.provider_id IS NOT NEW.provider_id
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id)
    SELECT ml.message_id FROM message_label ml WHERE ml.label_id = NEW.id
      AND EXISTS (SELECT 1 FROM item_source s WHERE s.message_id = ml.message_id AND s.role = 'origin');
END;

CREATE TRIGGER attention_due_message AFTER UPDATE OF hostile, internal_date ON message
  WHEN OLD.hostile IS NOT NEW.hostile OR OLD.internal_date IS NOT NEW.internal_date
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT NEW.id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = NEW.id AND role = 'origin');
END;

-- The sender or the date in the header.
CREATE TRIGGER attention_due_header_add AFTER INSERT ON header
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT NEW.message_id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = NEW.message_id AND role = 'origin');
END;

CREATE TRIGGER attention_due_header AFTER UPDATE OF from_addr, date ON header
  WHEN OLD.from_addr IS NOT NEW.from_addr OR OLD.date IS NOT NEW.date
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT NEW.message_id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = NEW.message_id AND role = 'origin');
END;

-- The cleanup of gone mail deletes the header: no sender, and maybe another date.
CREATE TRIGGER attention_due_header_remove AFTER DELETE ON header
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id) SELECT OLD.message_id
    WHERE EXISTS (SELECT 1 FROM item_source WHERE message_id = OLD.message_id AND role = 'origin');
END;

-- The kind of a sender: each message from that address. `from_addr` is the
-- lowercase address (spec 1 section 6.7), so the index of `from_addr` serves.
CREATE TRIGGER attention_due_sender_add AFTER INSERT ON sender
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id)
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(NEW.address)
      AND EXISTS (SELECT 1 FROM item_source s WHERE s.message_id = h.message_id AND s.role = 'origin');
END;

CREATE TRIGGER attention_due_sender AFTER UPDATE OF address, kind ON sender
  WHEN OLD.address IS NOT NEW.address OR OLD.kind IS NOT NEW.kind
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id)
    SELECT h.message_id FROM header h WHERE h.from_addr IN (lower(OLD.address), lower(NEW.address))
      AND EXISTS (SELECT 1 FROM item_source s WHERE s.message_id = h.message_id AND s.role = 'origin');
END;

CREATE TRIGGER attention_due_sender_remove AFTER DELETE ON sender
BEGIN
  INSERT OR IGNORE INTO attention_due(message_id)
    SELECT h.message_id FROM header h WHERE h.from_addr = lower(OLD.address)
      AND EXISTS (SELECT 1 FROM item_source s WHERE s.message_id = h.message_id AND s.role = 'origin');
END;
