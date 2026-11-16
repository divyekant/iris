-- Migration 8: the apply of an approved action (specification 2, section
-- 7.2, card R2-02). The trigger action_state of migration 7 gets the moves
-- approved to applied and approved to failed. The engine writes them after
-- the provider calls of the action. The action keeps the decision that
-- approved it, so the actor of an applied action is the rule or the person
-- that approved it.
--
-- `applying` marks an approved action whose write calls can have started:
-- the apply sets it before the first write call. It is the `writing`
-- exception of section 5.5 before the ledger: a withdraw keeps such an
-- action approved, it does not expire and gets no other decision, and it
-- ends as applied or failed.
--
-- The ledger migration (R2-03) replaces `applying` with the ledger row in
-- `writing`, adds the ledger entry check to these two moves, and the moves
-- to undone (issue #157). Until then, an applied action has no ledger
-- entry.

ALTER TABLE changeset_action ADD COLUMN applying INTEGER NOT NULL DEFAULT 0
  CHECK (applying IN (0, 1));

-- `applying` goes from 0 to 1 only, on an approved action of a changeset
-- that is not withdrawn.
CREATE TRIGGER action_applying BEFORE UPDATE OF applying ON changeset_action
WHEN new.applying IS NOT old.applying
  AND (old.applying <> 0 OR old.state IS NOT 'approved' OR new.state IS NOT 'approved'
       OR (SELECT state FROM changeset WHERE id = new.changeset_id) = 'withdrawn')
BEGIN
  SELECT RAISE(ABORT, 'iris: only the apply of an approved action marks it');
END;

CREATE TRIGGER action_applying_insert BEFORE INSERT ON changeset_action
WHEN new.applying <> 0
BEGIN
  SELECT RAISE(ABORT, 'iris: only the apply of an approved action marks it');
END;

DROP TRIGGER changeset_withdraw;

CREATE TRIGGER changeset_withdraw AFTER UPDATE OF state ON changeset
WHEN new.state = 'withdrawn' AND old.state <> 'withdrawn'
BEGIN
  UPDATE changeset_action SET state = 'cancelled', decided_by = 'author', rule = NULL,
    policy = NULL, reason = NULL, decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE changeset_id = new.id
    AND (state IS NULL OR state IN ('proposed', 'in_review')
         OR (state = 'approved' AND applying = 0));
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
    'approved>applied', 'approved>failed')
  -- An applied or failed action keeps the decision that approved it
  -- (section 5.1): the apply is not a decision.
  OR (new.state IN ('applied', 'failed')
      AND (new.decided_by IS NOT old.decided_by OR new.rule IS NOT old.rule
           OR new.policy IS NOT old.policy OR new.reason IS NOT old.reason
           OR new.decided_at IS NOT old.decided_at))
  -- An action in review waits for a person (section 5.2).
  OR (old.state = 'in_review' AND new.state IN ('approved', 'denied')
      AND new.decided_by IS NOT 'person')
  -- An action whose provider calls started ends as applied or failed.
  OR (old.state = 'approved' AND old.applying = 1 AND new.state NOT IN ('applied', 'failed'))
  -- A withdrawn changeset applies nothing more (section 5.5), except an
  -- action whose provider calls started.
  OR (new.state IN ('in_review', 'approved', 'applied', 'failed')
      AND (SELECT state FROM changeset WHERE id = new.changeset_id) = 'withdrawn'
      AND NOT (old.applying = 1 AND new.state IN ('applied', 'failed')))
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
