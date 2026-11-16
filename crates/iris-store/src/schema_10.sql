-- Migration 10: the ledger (specification 2, section 7, card R2-03).
--
-- The table `ledger` has one row for each applied action, and
-- `ledger_message` one row for each message of the row. The ledger row in
-- the state `writing` replaces the mark `applying` of migration 9: a
-- withdraw keeps an approved action with such a row, and the action ends
-- only as applied or failed.
--
-- The engine writes only the state of the ledger row. The trigger
-- ledger_state moves the action to the same state in the same write, so
-- the state of an applied, failed or undone action always agrees with its
-- ledger row.
--
-- The changeset, the revision, the verb, the author and the decision of an
-- entry come from its action row, which does not change after the submit.

CREATE TABLE ledger (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- One entry for each action. An undo is an action of its own.
  action_id INTEGER NOT NULL UNIQUE REFERENCES changeset_action(id),
  -- The mailbox of the provider calls. NULL for a state verb.
  account_id INTEGER REFERENCES account(id),
  -- JSON: the labels to add and remove, the call kind and the number of
  -- calls. For a state verb, the field.
  ops TEXT NOT NULL CHECK (json_valid(ops)),
  -- For a state verb only: the old and the new values, as JSON.
  pre TEXT CHECK (pre IS NULL OR json_valid(pre)),
  post TEXT CHECK (post IS NULL OR json_valid(post)),
  -- The number of the last provider call that the engine started, from 1.
  -- 0 before the first call.
  call INTEGER NOT NULL DEFAULT 0 CHECK (call >= 0),
  state TEXT NOT NULL CHECK (state IN ('writing', 'applied', 'failed', 'undone')),
  -- The refusal of the provider, or why the engine could not confirm.
  error TEXT,
  -- The Gmail history ID after the apply.
  history_id TEXT,
  -- 'inverse': an undo exists. 'none': final.
  undo TEXT NOT NULL CHECK (undo IN ('inverse', 'none')),
  -- The entry of the undo.
  undone_by INTEGER REFERENCES ledger(id),
  -- The time of the entry, in milliseconds.
  at INTEGER NOT NULL,
  CHECK ((state = 'undone') = (undone_by IS NOT NULL))
);

-- Recovery (section 7.3) finds the rows in `writing`.
CREATE INDEX ledger_state ON ledger(state);

-- No foreign key to message: the cleanup of gone messages deletes message
-- rows, and the apply and the recovery treat such a message as gone.
CREATE TABLE ledger_message (
  ledger_id INTEGER NOT NULL REFERENCES ledger(id),
  message_id INTEGER NOT NULL,
  -- The number of the provider call that changes the message, from 1.
  call INTEGER NOT NULL CHECK (call >= 1),
  -- JSON arrays: the labels that the action changes, before the action, and
  -- as the engine expects them after the action (section 7.1).
  pre TEXT NOT NULL CHECK (json_valid(pre)),
  post TEXT NOT NULL CHECK (json_valid(post)),
  -- 1 when the provider confirmed the change of the message.
  done INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
  -- Why the message has no `done` (section 7.3): 'skipped' (its call never
  -- started, and it already had the target state), 'gone' (HTTP 404 or
  -- gone in the mirror), 'unknown' (its call started, and recovery cannot
  -- confirm the change). An 'unknown' message makes the row failed.
  listed TEXT CHECK (listed IN ('skipped', 'gone', 'unknown')),
  CHECK (done = 0 OR listed IS NULL),
  PRIMARY KEY (ledger_id, message_id)
) WITHOUT ROWID;

-- Two rows in `writing` never share a message (section 7.2).
CREATE INDEX ledger_message_message ON ledger_message(message_id);

-- The data of migration 9. An action with the mark `applying` stopped
-- after a provider call started, and nothing lists its calls: it becomes
-- failed, with no undo. An applied or failed action of migration 9 gets an
-- entry with no messages and no undo. The triggers of migration 9 allow
-- these moves.
INSERT INTO ledger(action_id, ops, state, error, undo, at)
SELECT id, '{}', 'failed',
  'An apply stopped after a call to Gmail started, before the ledger. Iris does not call Gmail again for this action.',
  'none', CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM changeset_action WHERE state = 'approved' AND applying = 1;

UPDATE changeset_action SET state = 'failed' WHERE state = 'approved' AND applying = 1;

INSERT INTO ledger(action_id, ops, state, undo, at)
SELECT id, '{}', state, 'none', CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM changeset_action a
WHERE state IN ('applied', 'failed') AND NOT EXISTS (SELECT 1 FROM ledger WHERE action_id = a.id);

DROP TRIGGER action_applying;
DROP TRIGGER action_applying_insert;
DROP TRIGGER changeset_withdraw;
DROP TRIGGER action_state;
ALTER TABLE changeset_action DROP COLUMN applying;

-- A withdraw cancels each open action, except an approved action with a
-- ledger row in `writing` (section 5.5).
CREATE TRIGGER changeset_withdraw AFTER UPDATE OF state ON changeset
WHEN new.state = 'withdrawn' AND old.state <> 'withdrawn'
BEGIN
  UPDATE changeset_action SET state = 'cancelled', decided_by = 'author', rule = NULL,
    policy = NULL, reason = NULL, decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE changeset_id = new.id
    AND (state IS NULL OR state IN ('proposed', 'in_review')
         OR (state = 'approved'
             AND NOT EXISTS (SELECT 1 FROM ledger l
                             WHERE l.action_id = changeset_action.id AND l.state = 'writing')));
END;

CREATE TRIGGER action_state BEFORE UPDATE OF state ON changeset_action
WHEN coalesce(old.state, '') || '>' || coalesce(new.state, '') NOT IN (
    '>proposed', '>cancelled',
    'proposed>approved', 'proposed>in_review', 'proposed>denied', 'proposed>expired',
    'proposed>cancelled', 'proposed>replaced',
    'in_review>approved', 'in_review>denied', 'in_review>expired', 'in_review>cancelled',
    'in_review>replaced',
    'approved>in_review', 'approved>denied', 'approved>expired', 'approved>cancelled',
    'approved>applied', 'approved>failed', 'applied>undone', 'failed>undone')
  -- An applied, failed or undone action has its ledger entry in the same
  -- state (section 7.1). The trigger ledger_state writes this move.
  OR (new.state IN ('applied', 'failed', 'undone')
      AND NOT EXISTS (SELECT 1 FROM ledger WHERE action_id = new.id AND state = new.state))
  -- An applied, failed or undone action keeps the decision that approved
  -- it (section 5.1): the apply is not a decision.
  OR (new.state IN ('applied', 'failed', 'undone')
      AND (new.decided_by IS NOT old.decided_by OR new.rule IS NOT old.rule
           OR new.policy IS NOT old.policy OR new.reason IS NOT old.reason
           OR new.decided_at IS NOT old.decided_at))
  -- An action in review waits for a person (section 5.2).
  OR (old.state = 'in_review' AND new.state IN ('approved', 'denied')
      AND new.decided_by IS NOT 'person')
  -- An action whose provider calls started ends as applied or failed.
  OR (old.state = 'approved' AND new.state NOT IN ('applied', 'failed')
      AND EXISTS (SELECT 1 FROM ledger WHERE action_id = old.id AND state = 'writing'))
  -- A withdrawn changeset applies nothing more (section 5.5). An action
  -- with a ledger row ends as its row ends.
  OR (new.state IN ('in_review', 'approved')
      AND (SELECT state FROM changeset WHERE id = new.changeset_id) = 'withdrawn')
  OR (old.state IS NULL AND new.state <> 'cancelled'
      AND (new.hash IS NULL OR new.submitted_at IS NULL
           OR NOT EXISTS (SELECT 1 FROM changeset WHERE id = new.changeset_id
                          AND revision = new.revision
                          AND submitted_at IS new.submitted_at)))
  -- No approval while a new revision is a draft (section 5.5): its submit
  -- replaces the older actions with no decision.
  OR (new.state = 'approved'
      AND (SELECT submitted_at FROM changeset WHERE id = new.changeset_id) IS NULL)
  -- An action is cancelled only with a withdraw of its changeset.
  OR (new.state = 'cancelled'
      AND (SELECT state FROM changeset WHERE id = new.changeset_id) <> 'withdrawn')
  -- Replaced only in the submit of a newer revision (trigger
  -- changeset_submit). An older action that comes back to review after the
  -- submit stays for the person (section 5.5).
  OR (new.state = 'replaced'
      AND NOT EXISTS (SELECT 1 FROM changeset WHERE id = new.changeset_id
                      AND replacing = revision AND revision > new.revision
                      AND submitted_at IS new.decided_at))
  -- Expired only 14 days after its submit (section 5.5), and not at a time
  -- after the clock of the database.
  OR (new.state = 'expired'
      AND (new.decided_at IS NULL OR new.decided_at - old.submitted_at < 1209600000
           OR new.decided_at > unixepoch('subsec') * 1000))
  -- A rule decision names a rule ID of the active policy (section 6.3).
  OR (new.decided_by IS 'rule' AND new.state IN ('in_review', 'approved', 'denied')
      AND (new.policy IS NOT (SELECT max(version) FROM policy_version)
           OR NOT EXISTS (SELECT 1 FROM policy_rule_id
                          WHERE id = new.rule AND removed_at IS NULL)))
BEGIN
  SELECT RAISE(ABORT, 'iris: this state change of an action is not allowed');
END;

-- A new entry is the claim of an approved action, in `writing`, before
-- its first provider call.
CREATE TRIGGER ledger_insert BEFORE INSERT ON ledger
WHEN new.state <> 'writing' OR new.call <> 0 OR new.error IS NOT NULL
  OR new.history_id IS NOT NULL OR new.undone_by IS NOT NULL
  OR NOT EXISTS (SELECT 1 FROM changeset_action WHERE id = new.action_id AND state = 'approved')
BEGIN
  SELECT RAISE(ABORT, 'iris: a new ledger entry claims an approved action');
END;

-- An entry changes only to move its state forward, and to record the
-- progress of the provider calls (section 7.1).
CREATE TRIGGER ledger_change BEFORE UPDATE ON ledger
WHEN new.id IS NOT old.id OR new.action_id IS NOT old.action_id
  OR new.account_id IS NOT old.account_id OR new.ops IS NOT old.ops
  OR new.pre IS NOT old.pre OR new.post IS NOT old.post
  OR new.undo IS NOT old.undo OR new.at IS NOT old.at
  OR (new.state IS NOT old.state
      AND old.state || '>' || new.state NOT IN ('writing>applied', 'writing>failed',
                                                'applied>undone', 'failed>undone'))
  -- The calls, the error and the history ID change only in `writing`.
  OR (old.state <> 'writing'
      AND (new.call IS NOT old.call OR new.error IS NOT old.error
           OR new.history_id IS NOT old.history_id))
  OR new.call < old.call
  -- The undo of an entry names the entry of the undo: a later entry that
  -- ended, and that undoes no other entry. A final entry (undo 'none') has
  -- no undo. A failed entry has an undo only for the messages that the
  -- provider changed (section 5.2).
  OR (new.undone_by IS NOT old.undone_by
      AND (new.state IS NOT 'undone' OR old.state = 'undone' OR old.undo IS NOT 'inverse'
           OR new.undone_by <= old.id
           OR coalesce((SELECT state FROM ledger WHERE id = new.undone_by), '')
                NOT IN ('applied', 'failed')
           OR EXISTS (SELECT 1 FROM ledger WHERE undone_by = new.undone_by)))
  OR (new.state = 'undone' AND old.state = 'failed'
      AND NOT EXISTS (SELECT 1 FROM ledger_message WHERE ledger_id = old.id AND done = 1))
BEGIN
  SELECT RAISE(ABORT, 'iris: this change of a ledger entry is not allowed');
END;

-- The action follows its entry.
CREATE TRIGGER ledger_state AFTER UPDATE OF state ON ledger
WHEN new.state IS NOT old.state
BEGIN
  UPDATE changeset_action SET state = new.state WHERE id = new.action_id;
END;

CREATE TRIGGER ledger_delete BEFORE DELETE ON ledger
BEGIN
  SELECT RAISE(ABORT, 'iris: the ledger keeps each entry');
END;

-- The messages of an entry are written in the transaction of its claim,
-- before the first call. Each one is a resolved message of the action, and
-- no other entry in `writing` has it (section 7.2).
CREATE TRIGGER ledger_message_insert BEFORE INSERT ON ledger_message
WHEN new.done <> 0 OR new.listed IS NOT NULL
  OR NOT EXISTS (SELECT 1 FROM ledger WHERE id = new.ledger_id AND state = 'writing' AND call = 0)
  OR NOT EXISTS (SELECT 1 FROM action_message
                 WHERE message_id = new.message_id
                   AND action_id = (SELECT action_id FROM ledger WHERE id = new.ledger_id))
  OR EXISTS (SELECT 1 FROM ledger_message lm JOIN ledger l ON l.id = lm.ledger_id
             WHERE lm.message_id = new.message_id AND l.state = 'writing'
               AND l.id <> new.ledger_id)
BEGIN
  SELECT RAISE(ABORT, 'iris: this ledger message is not allowed');
END;

-- In `writing`, `done` and `listed` are set once. `pre` changes only for a
-- message whose call never started: the recovery reads it again (section
-- 7.3).
CREATE TRIGGER ledger_message_change BEFORE UPDATE ON ledger_message
WHEN new.ledger_id IS NOT old.ledger_id OR new.message_id IS NOT old.message_id
  OR new.call IS NOT old.call OR new.post IS NOT old.post
  OR NOT EXISTS (SELECT 1 FROM ledger WHERE id = old.ledger_id AND state = 'writing')
  OR (new.done IS NOT old.done AND old.done <> 0)
  OR (new.listed IS NOT old.listed AND old.listed IS NOT NULL)
  OR (new.pre IS NOT old.pre
      AND old.call <= (SELECT call FROM ledger WHERE id = old.ledger_id))
BEGIN
  SELECT RAISE(ABORT, 'iris: this change of a ledger message is not allowed');
END;

CREATE TRIGGER ledger_message_delete BEFORE DELETE ON ledger_message
BEGIN
  SELECT RAISE(ABORT, 'iris: the ledger keeps each message');
END;
