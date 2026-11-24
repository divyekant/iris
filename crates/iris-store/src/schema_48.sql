-- Migration 48: who withdrew a changeset (decision D16, issue #221).
-- 'author': the author of the changeset withdrew it. 'person': you withdrew
-- it. NULL on a changeset that is not withdrawn, and on a changeset that a
-- version before migration 47 withdrew (the actor is not known).
ALTER TABLE changeset ADD COLUMN withdrawn_by TEXT
  CHECK (withdrawn_by IN ('author', 'person'));

-- Each new withdraw names who withdrew, in the same update as the state.
CREATE TRIGGER changeset_withdraw_by BEFORE UPDATE OF state ON changeset
WHEN new.state = 'withdrawn' AND old.state <> 'withdrawn' AND new.withdrawn_by IS NULL
BEGIN
  SELECT RAISE(ABORT, 'iris: a withdraw names who withdrew');
END;

-- withdrawn_by is set only with the withdraw, and does not change after.
CREATE TRIGGER changeset_withdrawn_by BEFORE UPDATE OF withdrawn_by ON changeset
WHEN new.withdrawn_by IS NOT old.withdrawn_by
  AND (old.state = 'withdrawn' OR new.state IS NOT 'withdrawn')
BEGIN
  SELECT RAISE(ABORT, 'iris: withdrawn_by is set only with the withdraw');
END;
