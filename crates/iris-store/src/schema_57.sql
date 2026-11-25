-- Migration 57: the fixed rules that completed the understanding of a
-- message (issue 946). The fixed check of `iris-understand` compares the
-- fixed result now with this record. It read the rule items in
-- `item_droppable` before, which leaves out a pinned item, so a pinned code
-- signal whose rule stopped applying did not make the mail wait for the
-- model.
--
-- `fixed_rules` is NULL when the model made an item of the last
-- understanding, else a JSON array of the rule of each rule item. A
-- message that is not understood can keep an old value: no reader uses it
-- then, and the next understanding writes it again.

ALTER TABLE message ADD COLUMN fixed_rules TEXT;

-- The backfill cannot see which run made each item. An item that is not
-- pinned (`item_droppable`) comes from the last run:
-- - a model item that is not pinned: the model completed it (NULL);
-- - a rule item that is not pinned: the rules completed it (the rule of
--   each rule item, pinned or not);
-- - else, with a pinned rule item: not known. The last run can be a rule
--   whose draft the pinned item holds, or a model run with no item. The
--   record is NULL, and the message goes to the fixed check
--   (`fixed_due`), which runs the fixed rules again with no model call and
--   writes the record when a rule applies now;
-- - else: no item of a rule or the model (`[]`).
UPDATE message SET fixed_rules = CASE
  WHEN EXISTS (SELECT 1 FROM item_droppable d JOIN item i ON i.id = d.item_id
               WHERE d.message_id = message.id AND i.derived_by = 'model') THEN NULL
  WHEN EXISTS (SELECT 1 FROM item_droppable d JOIN item i ON i.id = d.item_id
               WHERE d.message_id = message.id AND i.derived_by = 'rule')
    THEN (SELECT json_group_array(json_extract(r.doc, '$.derived.rule'))
            FROM item_source rs JOIN item r ON r.id = rs.item_id
           WHERE rs.message_id = message.id AND rs.role = 'origin'
             AND r.form <> 'conversation' AND r.derived_by = 'rule')
  WHEN EXISTS (SELECT 1 FROM item_source rs JOIN item r ON r.id = rs.item_id
               WHERE rs.message_id = message.id AND rs.role = 'origin'
                 AND r.form <> 'conversation' AND r.derived_by = 'rule') THEN NULL
  ELSE '[]' END
WHERE understood_at IS NOT NULL;

INSERT INTO fixed_due(message_id)
  SELECT m.id FROM message m
  WHERE m.understood_at IS NOT NULL AND m.fixed_rules IS NULL
    AND NOT EXISTS (SELECT 1 FROM item_droppable d JOIN item i ON i.id = d.item_id
                    WHERE d.message_id = m.id AND i.derived_by = 'model');

-- The revision of the gate of each sender (issue 934). Each change of
-- `declined_at` adds one, so a decline and a reopen during one model call
-- show as a change, also when `declined_at` is NULL again at the end. The
-- understanding runner compares the revision before and after a call.
ALTER TABLE sender ADD COLUMN gate_rev INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER sender_gate_rev AFTER UPDATE OF declined_at ON sender
  WHEN OLD.declined_at IS NOT NEW.declined_at
BEGIN
  UPDATE sender SET gate_rev = gate_rev + 1 WHERE id = NEW.id;
END;
