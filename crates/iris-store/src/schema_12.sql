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
-- submit. The entry has an undo and is not undone, the verb fits the step,
-- and no action of another undo of the entry is open.
CREATE TRIGGER ledger_undo_insert BEFORE INSERT ON ledger_undo
WHEN NOT EXISTS (SELECT 1 FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.id = new.action_id AND a.state IS NULL AND c.author_kind = 'person')
  OR NOT EXISTS (SELECT 1 FROM ledger WHERE id = new.ledger_id
                   AND state IN ('applied', 'failed') AND undo = 'inverse')
  -- Step 1 is the inverse of the verb of the entry, with the same
  -- arguments. Step 2 is the `archive` after the `untrash` of a `trash`.
  OR NOT EXISTS (SELECT 1 FROM changeset_action a, ledger l
                 JOIN changeset_action o ON o.id = l.action_id
                 WHERE a.id = new.action_id AND l.id = new.ledger_id
                   AND ((new.step = 1 AND json(a.args) = json(o.args)
                         AND o.verb || '>' || a.verb IN (
                           'archive>inbox', 'inbox>archive', 'read>unread', 'unread>read',
                           'label>unlabel', 'unlabel>label', 'trash>untrash', 'untrash>trash'))
                        OR (new.step = 2 AND o.verb = 'trash' AND a.verb = 'archive'
                            -- Step 1 of the same entry, in the same changeset.
                            AND EXISTS (SELECT 1 FROM ledger_undo u1
                                        JOIN changeset_action a1 ON a1.id = u1.action_id
                                        WHERE u1.ledger_id = new.ledger_id AND u1.step = 1
                                          AND a1.changeset_id = a.changeset_id))))
  -- One open undo of an entry: no open action of another undo changeset,
  -- and one action of each step in the same changeset.
  OR EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
             JOIN changeset_action na ON na.id = new.action_id
             WHERE u.ledger_id = new.ledger_id
               AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved')
               AND (a.changeset_id <> na.changeset_id OR u.step = new.step))
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

-- The link checks the verb and the arguments of the action. They do not
-- change after the link, also in a draft, so the check stays true.
CREATE TRIGGER ledger_undo_action BEFORE UPDATE OF verb, targets, args, changeset_id
ON changeset_action
WHEN EXISTS (SELECT 1 FROM ledger_undo WHERE action_id = old.id)
  AND (new.verb IS NOT old.verb OR new.targets IS NOT old.targets
       OR new.args IS NOT old.args OR new.changeset_id IS NOT old.changeset_id)
BEGIN
  SELECT RAISE(ABORT, 'iris: the action of an undo link does not change');
END;

-- The entries that have an undo now (section 7.1): applied or failed, with
-- an undo, a changed message that is not gone, and no open undo. After
-- Gmail deleted each message of a `trash` entry, the entry is final.
CREATE VIEW ledger_can_undo AS
  SELECT l.id AS ledger_id FROM ledger l
  WHERE l.state IN ('applied', 'failed') AND l.undo = 'inverse'
    AND EXISTS (SELECT 1 FROM ledger_message lm
                JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
                WHERE lm.ledger_id = l.id AND lm.done = 1)
    AND NOT EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
                    WHERE u.ledger_id = l.id
                      AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved'));
