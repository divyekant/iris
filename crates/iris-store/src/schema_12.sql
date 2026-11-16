-- Migration 12: the undo of a ledger entry (specification 2, section 7.4,
-- card R2-03).
--
-- An undo is an ordinary action of a changeset of the person, with its own
-- ledger entry. The table `ledger_undo` links each such action to the entry
-- that it undoes. Step 1 is the inverse of the entry. Step 2 exists only
-- for the undo of `trash`: an `archive` of each message that `untrash` put
-- in the inbox, and that had no `INBOX` before the trash.

CREATE TABLE ledger_undo (
  action_id INTEGER PRIMARY KEY REFERENCES changeset_action(id),
  ledger_id INTEGER NOT NULL REFERENCES ledger(id),
  step INTEGER NOT NULL CHECK (step IN (1, 2))
);

CREATE INDEX ledger_undo_ledger ON ledger_undo(ledger_id);

-- The link is written with the draft action of the person, before the
-- submit. The entry has an undo and is not undone, and no other open undo
-- of the entry exists.
CREATE TRIGGER ledger_undo_insert BEFORE INSERT ON ledger_undo
WHEN NOT EXISTS (SELECT 1 FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.id = new.action_id AND a.state IS NULL AND c.author_kind = 'person')
  OR NOT EXISTS (SELECT 1 FROM ledger WHERE id = new.ledger_id
                   AND state IN ('applied', 'failed') AND undo = 'inverse')
  OR EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
             WHERE u.ledger_id = new.ledger_id AND u.step = new.step
               AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved'))
BEGIN
  SELECT RAISE(ABORT, 'iris: this undo link is not allowed');
END;

CREATE TRIGGER ledger_undo_change BEFORE UPDATE ON ledger_undo
BEGIN
  SELECT RAISE(ABORT, 'iris: an undo link does not change');
END;

CREATE TRIGGER ledger_undo_delete BEFORE DELETE ON ledger_undo
BEGIN
  SELECT RAISE(ABORT, 'iris: an undo link does not change');
END;

-- An entry becomes `undone` only by the entry of step 1 of its own undo.
-- The trigger ledger_change of migration 11 checks the rest of the move.
CREATE TRIGGER ledger_undone_by BEFORE UPDATE OF undone_by ON ledger
WHEN new.undone_by IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM ledger u JOIN ledger_undo lu ON lu.action_id = u.action_id
                  WHERE u.id = new.undone_by AND lu.ledger_id = new.id AND lu.step = 1)
BEGIN
  SELECT RAISE(ABORT, 'iris: an entry is undone only by its own undo');
END;

-- The two steps of the undo of one `trash`: `second` is the `archive` of
-- step 2, and `first` is the `untrash` of step 1 in the same changeset.
CREATE VIEW undo_pair AS
  SELECT u2.action_id AS second, u1.action_id AS first
  FROM ledger_undo u2
  JOIN changeset_action a2 ON a2.id = u2.action_id
  JOIN changeset_action a1 ON a1.changeset_id = a2.changeset_id
  JOIN ledger_undo u1 ON u1.action_id = a1.id AND u1.step = 1
  WHERE u2.step = 2;
