-- Migration 57: the undo of a change of a draft
-- (docs/specs/2026-11-25-draft-change.md, section 3).
--
-- 1. The trigger ledger_undo_insert accepts a `draft` action as the undo of
--    an applied change of a draft (`drafts.update`) whose call started. Its
--    arguments are the content of `pre`. Each rule of migration 29 stays.
-- 2. The view ledger_can_undo lists such an entry while it is the newest
--    entry of its draft that counts, and the draft is not sent, not in a
--    form that Iris does not write, and has no claimed send. A failed later
--    change counts only with the two errors of an unknown result (the
--    constants CHANGE_UNKNOWN and CHANGE_DURING of iris-apply; a test
--    compares them). Each rule of migration 29 stays.

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
                        -- The undo of a change of a draft: a `draft` action whose
                        -- arguments are the content before the change. A reply
                        -- takes no subject: the draft keeps its subject.
                        OR (new.step = 1 AND o.verb = 'draft' AND a.verb = 'draft'
                            AND json_extract(l.ops, '$.call') = 'drafts.update'
                            AND l.state = 'applied' AND l.call = 1
                            AND json(a.args) = CASE
                              WHEN json_extract(l.ops, '$.thread') IS NULL
                                THEN json(json_extract(l.pre, '$.draft.content'))
                              ELSE json_remove(json_extract(l.pre, '$.draft.content'), '$.subject') END)
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
                      AND json_type(l.ops, '$.corrections') = 'array')
         OR (json_extract(l.ops, '$.call') = 'drafts.update' AND l.state = 'applied' AND l.call = 1
             -- Checks of section 1 that the store shows: the draft is not
             -- sent, is in a form that Iris writes, and has no claimed send.
             AND EXISTS (SELECT 1 FROM draft d WHERE d.id = json_extract(l.ops, '$.draft')
                           AND d.sent_at IS NULL AND d.foreign_at IS NULL)
             AND NOT EXISTS (SELECT 1 FROM ledger_send s
                             WHERE s.draft_id = json_extract(l.ops, '$.draft'))
             -- The newest entry of the draft. A later change or undo
             -- counts after its call started, unless Gmail rejected it.
             AND NOT EXISTS (SELECT 1 FROM ledger n
                             WHERE n.id > l.id AND json_extract(n.ops, '$.call') = 'drafts.update'
                               AND json_extract(n.ops, '$.draft') = json_extract(l.ops, '$.draft')
                               AND (n.state = 'writing'
                                    OR (n.state IN ('applied', 'undone') AND n.call = 1)
                                    OR (n.state = 'failed' AND n.error IN (
                                      'Iris does not know if the draft changed. Look at it in Gmail.',
                                      'The draft changed at Gmail during the change. Look at it in Gmail.'))))))
    AND NOT EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action a ON a.id = u.action_id
                    WHERE u.ledger_id = l.id
                      AND coalesce(a.state, 'proposed') IN ('proposed', 'in_review', 'approved'));
