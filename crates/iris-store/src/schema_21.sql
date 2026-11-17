-- Migration 21: the held outbox, the cancel and the send switch (card
-- R3-02, docs/specs/2026-11-17-held-outbox.md).
--
-- A send moves from `approved` to `held`, and leaves `held` by the release
-- (`applied`, `failed`, `denied` by `engine`), by the cancel of a person
-- (`denied` by `person`, reason "Cancelled in the outbox."), by a withdraw
-- (`cancelled`) or by the expiry. A claimed send (a ledger row in
-- `writing`) ends only as its row ends.

DROP TRIGGER changeset_withdraw;

CREATE TRIGGER changeset_withdraw AFTER UPDATE OF state ON changeset
WHEN new.state = 'withdrawn' AND old.state <> 'withdrawn'
BEGIN
  UPDATE changeset_action SET state = 'cancelled', decided_by = 'author', rule = NULL,
    policy = NULL, reason = NULL, decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE changeset_id = new.id
    AND (state IS NULL OR state IN ('proposed', 'in_review')
         OR (state IN ('approved', 'held')
             AND NOT EXISTS (SELECT 1 FROM ledger l
                             WHERE l.action_id = changeset_action.id AND l.state = 'writing')));
END;

DROP TRIGGER action_state;

CREATE TRIGGER action_state BEFORE UPDATE OF state ON changeset_action
WHEN coalesce(old.state, '') || '>' || coalesce(new.state, '') NOT IN (
    '>proposed', '>cancelled',
    'proposed>approved', 'proposed>in_review', 'proposed>denied', 'proposed>expired',
    'proposed>cancelled', 'proposed>replaced',
    'in_review>approved', 'in_review>denied', 'in_review>expired', 'in_review>cancelled',
    'in_review>replaced',
    'approved>in_review', 'approved>denied', 'approved>expired', 'approved>cancelled',
    'approved>applied', 'approved>failed', 'applied>undone', 'failed>undone',
    -- The held outbox (docs/specs/2026-11-17-held-outbox.md, section 1).
    'approved>held', 'held>applied', 'held>failed', 'held>denied', 'held>expired',
    'held>cancelled')
  -- Only a send waits in the held outbox.
  OR (new.state = 'held' AND new.verb <> 'send')
  -- A held send keeps the decision that approved it.
  OR (old.state = 'approved' AND new.state = 'held'
      AND (new.decided_by IS NOT old.decided_by OR new.rule IS NOT old.rule
           OR new.policy IS NOT old.policy OR new.reason IS NOT old.reason
           OR new.decided_at IS NOT old.decided_at))
  -- A held send is denied by the release (engine) or by the cancel of a
  -- person (the reason says so).
  OR (old.state = 'held' AND new.state = 'denied'
      AND new.decided_by NOT IN ('engine', 'person'))
  OR (new.state = 'denied' AND new.decided_by = 'person' AND new.verb = 'send'
      AND old.state IN ('approved', 'held') AND new.reason IS NOT 'Cancelled in the outbox.')
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
  OR (old.state IN ('approved', 'held') AND new.state NOT IN ('applied', 'failed')
      AND EXISTS (SELECT 1 FROM ledger WHERE action_id = old.id AND state = 'writing'))
  -- A withdrawn changeset applies nothing more (section 5.5). An action
  -- with a ledger row ends as its row ends.
  OR (new.state IN ('in_review', 'approved', 'held')
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

DROP TRIGGER ledger_insert;

CREATE TRIGGER ledger_insert BEFORE INSERT ON ledger
WHEN new.state <> 'writing' OR new.call <> 0 OR new.error IS NOT NULL
  OR new.history_id IS NOT NULL OR new.undone_by IS NOT NULL
  OR NOT EXISTS (SELECT 1 FROM changeset_action WHERE id = new.action_id
                 AND (state = 'approved' OR (state = 'held' AND verb = 'send')))
BEGIN
  SELECT RAISE(ABORT, 'iris: a new ledger entry claims an approved action or a held send');
END;

-- The send switch: one row. `on_at` is NULL while sending is off. Only the
-- owner turns it on (`iris sending on`).
CREATE TABLE send_switch (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  on_at INTEGER CHECK (on_at IS NULL OR (typeof(on_at) = 'integer' AND on_at >= 0)),
  changed_at INTEGER NOT NULL CHECK (typeof(changed_at) = 'integer' AND changed_at >= 0)
);
INSERT INTO send_switch(id, on_at, changed_at) VALUES (1, NULL, 0);
