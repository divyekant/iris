-- Migration 27: the verbs `skip`, `mute` and `dismiss` of the person
-- (docs/specs/2026-11-17-preference-rules.md, card R5-02).
--
-- 1. The three verbs join the verb CHECK of changeset_action, with the
--    method of migration 18: a wider CHECK accepts each stored row, so only
--    the text of the table changes in sqlite_schema. They go before
--    'draft', so the text keeps the end that the check of migration 26
--    reads.
-- 2. An entry of the three verbs is a state entry: the trigger
--    ledger_undo_insert and the view ledger_can_undo of migration 26 accept
--    its undo (`restore`), as for `close`.

PRAGMA writable_schema = ON;
UPDATE sqlite_schema
SET sql = replace(sql, '''correct'', ''draft'', ''send'', ''restore''))',
                  '''correct'', ''skip'', ''mute'', ''dismiss'', ''draft'', ''send'', ''restore''))')
WHERE type = 'table' AND name = 'changeset_action';
PRAGMA writable_schema = RESET;

-- Stop the migration if the text above did not change: the CHECK of a
-- temporary table refuses 0.
CREATE TEMP TABLE migration_27 (ok INTEGER CHECK (ok = 1));
INSERT INTO migration_27
SELECT instr(sql, '''skip'', ''mute'', ''dismiss'', ''draft'', ''send'', ''restore'')') > 0
FROM sqlite_schema WHERE type = 'table' AND name = 'changeset_action';
DROP TABLE migration_27;

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
                        -- An entry from before migration 26 has no IDs of its
                        -- correction rows, so it has no undo.
                        OR (new.step = 1 AND json(a.args) = '{}' AND a.verb = 'restore'
                            AND o.verb IN ('close', 'decline', 'defer', 'correct', 'skip', 'mute', 'dismiss')
                            AND json_type(l.ops, '$.corrections') = 'array')
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
                      AND o.verb IN ('close', 'decline', 'defer', 'correct', 'skip', 'mute', 'dismiss')
                      AND json_type(l.ops, '$.corrections') = 'array'))
    AND NOT EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
                    WHERE u.ledger_id = l.id
                      AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved'));
