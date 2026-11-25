-- Migration 55: the fixed rules that completed the understanding of a
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

UPDATE message SET fixed_rules = (
  SELECT CASE WHEN max(i.derived_by = 'model') THEN NULL
    ELSE json_group_array(json_extract(i.doc, '$.derived.rule')) END
  FROM item_source src JOIN item i ON i.id = src.item_id
  WHERE src.message_id = message.id AND src.role = 'origin'
    AND i.form <> 'conversation' AND i.derived_by IN ('rule', 'model'))
WHERE understood_at IS NOT NULL;

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
