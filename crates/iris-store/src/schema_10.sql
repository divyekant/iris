-- Specification 2, section 4: the items of an action of a state verb
-- (`close`, `defer`, `decline`, `correct`), at submit. Submit writes these
-- rows for a draft action. No foreign key to item: understanding deletes
-- and makes item rows, and `item_pin` keeps each item that an open action
-- names.
CREATE TABLE action_item (
  action_id INTEGER NOT NULL REFERENCES changeset_action(id),
  item_id INTEGER NOT NULL,
  PRIMARY KEY (action_id, item_id)
) WITHOUT ROWID;
CREATE INDEX action_item_item ON action_item(item_id);

-- A submitted action does not change (migration 7, `action_message`).
CREATE TRIGGER action_item_insert BEFORE INSERT ON action_item
WHEN (SELECT state FROM changeset_action WHERE id = new.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

CREATE TRIGGER action_item_delete BEFORE DELETE ON action_item
WHEN (SELECT state FROM changeset_action WHERE id = old.action_id) IS NOT NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- Submit inserts and deletes these rows. Nothing updates them.
CREATE TRIGGER action_item_update BEFORE UPDATE ON action_item
BEGIN
  SELECT RAISE(ABORT, 'iris: a submitted action does not change');
END;

-- The pinned items (specification 1, section 7.5): an item with a
-- correction, or an item that an open action names. Understanding never
-- deletes a pinned item and never changes its ID. The ledger (R2-03) adds
-- its references here.
CREATE VIEW item_pin AS
  SELECT item_id FROM correction
  UNION
  SELECT ai.item_id FROM action_item ai
  JOIN changeset_action a ON a.id = ai.action_id
  WHERE a.state IN ('proposed', 'in_review', 'approved', 'held');
