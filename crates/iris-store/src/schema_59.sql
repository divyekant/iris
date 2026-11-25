-- Migration 59: the hostile evidence of a send at its claim (issue 472).
--
-- The measure "sends without review after hostile mail" must not change
-- when a sender kind changes later or when the removal of an account
-- deletes its messages. The send claim writes the result of
-- `iris_changeset::hostile_evidence` here: 1 if the thread of the draft
-- had hostile evidence at the claim, 0 if not. NULL for each other ledger
-- row, and for each send claimed before this migration.
ALTER TABLE ledger ADD COLUMN hostile_before INTEGER
  CHECK (hostile_before IS NULL OR hostile_before IN (0, 1));

-- The value is a record of the claim: it does not change.
CREATE TRIGGER ledger_hostile_before BEFORE UPDATE OF hostile_before ON ledger
WHEN new.hostile_before IS NOT old.hostile_before
BEGIN
  SELECT RAISE(ABORT, 'iris: the hostile evidence of a send does not change');
END;
