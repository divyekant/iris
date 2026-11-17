-- Migration 25: limits for hostile input (docs/specs/2026-11-17-hostile-input-limits.md,
-- card R4-06).
--
-- Only a person approves a draft or a send with a warning flag (spec 2,
-- section 6.5). The check reads the new row on a change of the state or of
-- the flags: a move to approved, and a flag that an update adds to an
-- action that a rule approved (trigger action_flags allows it). A held,
-- applied or failed send keeps the decision that approved it (migration
-- 22), so this check on `approved` covers them.
CREATE TRIGGER action_flagged_write BEFORE UPDATE OF state, flags ON changeset_action
WHEN new.state = 'approved' AND new.verb IN ('draft', 'send')
  AND new.decided_by IS NOT 'person'
  AND EXISTS (SELECT 1 FROM json_each(new.flags)
              WHERE value IN ('new_recipient', 'outside_thread', 'money', 'date'))
BEGIN
  SELECT RAISE(ABORT, 'iris: only a person approves a draft or a send with a warning flag');
END;
