-- Migration 24: the undo of a state entry (specification 2, sections 4
-- and 7.4, issue #226).
--
-- 1. The verb `restore` joins the verb CHECK of changeset_action, as in
--    migration 18. Only an undo makes it: it sets the `pre` values of a
--    `close`, `decline`, `defer` or `correct` entry again.
-- 2. The trigger ledger_undo_insert accepts `restore` with no arguments as
--    step 1 of the undo of a state entry.
-- 3. The view ledger_can_undo lists a state entry with one or more items in
--    `post`: a state entry changes no message.

PRAGMA writable_schema = ON;
UPDATE sqlite_schema
SET sql = replace(sql, '''draft'', ''send''))', '''draft'', ''send'', ''restore''))')
WHERE type = 'table' AND name = 'changeset_action';
PRAGMA writable_schema = RESET;

CREATE TEMP TABLE migration_24 (ok INTEGER CHECK (ok = 1));
INSERT INTO migration_24
SELECT instr(sql, '''send'', ''restore'')') > 0
FROM sqlite_schema WHERE type = 'table' AND name = 'changeset_action';
DROP TABLE migration_24;

DROP TRIGGER ledger_undo_insert;

CREATE TRIGGER ledger_undo_insert BEFORE INSERT ON ledger_undo
WHEN NOT EXISTS (SELECT 1 FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                 WHERE a.id = new.action_id AND a.state IS NULL AND c.author_kind = 'person')
  OR NOT EXISTS (SELECT 1 FROM ledger WHERE id = new.ledger_id
                   AND state IN ('applied', 'failed') AND undo = 'inverse')
  -- The target is the entry. Step 1 is the inverse of the verb of the
  -- entry, with the same arguments. Step 2 is the `archive` after the `untrash` of a `trash`.
  OR NOT EXISTS (SELECT 1 FROM changeset_action a, ledger l
                 JOIN changeset_action o ON o.id = l.action_id
                 WHERE a.id = new.action_id AND l.id = new.ledger_id
                   -- The target is the whole entry.
                   AND json(a.targets) = json_object('ledger', json_array('l-' || l.id))
                   AND ((new.step = 1 AND json(a.args) = json(o.args)
                         AND o.verb || '>' || a.verb IN (
                           'archive>inbox', 'inbox>archive', 'read>unread', 'unread>read',
                           'label>unlabel', 'unlabel>label', 'trash>untrash', 'untrash>trash'))
                        -- The undo of a state entry: `restore`, with no arguments.
                        OR (new.step = 1 AND json(a.args) = '{}' AND a.verb = 'restore'
                            AND o.verb IN ('close', 'decline', 'defer', 'correct'))
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

DROP VIEW ledger_can_undo;

CREATE VIEW ledger_can_undo AS
  SELECT l.id AS ledger_id FROM ledger l
  WHERE l.state IN ('applied', 'failed') AND l.undo = 'inverse'
    AND (EXISTS (SELECT 1 FROM ledger_message lm
                 JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
                 WHERE lm.ledger_id = l.id AND lm.done = 1)
         -- A state entry changed items, not messages: its `post` holds them.
         OR EXISTS (SELECT 1 FROM changeset_action o, json_each(l.post) p
                    WHERE o.id = l.action_id
                      AND o.verb IN ('close', 'decline', 'defer', 'correct')))
    AND NOT EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
                    WHERE u.ledger_id = l.id
                      AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved'));
