-- Migration 45 (issue #494): the measure "proposed actions that you approve
-- without an edit" counts an approval as without an edit only if the person
-- wrote no comment on an earlier revision of the changeset. An approval of
-- a revision that came after a comment of the person is a decision, but
-- not an approval of the first proposal. A revision of an author with no
-- comment of the person (for example triage, which makes a new revision
-- when its own proposal changes) still counts as without an edit.

DROP TRIGGER person_decision;

CREATE TRIGGER person_decision AFTER UPDATE OF state ON changeset_action
WHEN new.decided_by IS 'person' AND new.state IN ('approved', 'denied')
  AND new.state IS NOT old.state
  AND (SELECT author_kind FROM changeset WHERE id = new.changeset_id) <> 'person'
BEGIN
  DELETE FROM person_decision WHERE action_id = new.id;
  INSERT INTO person_decision(action_id, approved, at)
  VALUES (
    new.id,
    new.state = 'approved'
      AND NOT EXISTS (SELECT 1 FROM changeset_comment cc
                      WHERE cc.changeset_id = new.changeset_id
                        AND cc.revision < new.revision),
    coalesce(new.decided_at, 0));
END;

-- The approvals that the old trigger counted as without an edit.
UPDATE person_decision SET approved = 0
WHERE approved = 1 AND EXISTS (
  SELECT 1 FROM changeset_action a JOIN changeset_comment cc ON cc.changeset_id = a.changeset_id
  WHERE a.id = person_decision.action_id AND cc.revision < a.revision);
