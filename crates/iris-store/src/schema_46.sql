-- Migration 46: matters that the person edits, signal members and
-- proposals of additions (docs/specs/2026-11-24-matters-edit.md, decisions
-- D19 to D22). The triggers below replace the matter triggers of migration
-- 16 and hold the rules of the spec ("Store: migration 46").

-- Who made the matter: the proposal step, an agent, or the person.
ALTER TABLE matter ADD COLUMN author TEXT NOT NULL DEFAULT 'model'
  CHECK (author IN ('model', 'agent', 'person'));

-- One pending addition for each item: Iris proposes an item to one
-- confirmed matter at most.
CREATE TABLE matter_add (
  item_id INTEGER PRIMARY KEY REFERENCES item(id) ON DELETE CASCADE,
  matter_id INTEGER NOT NULL REFERENCES matter(id) ON DELETE CASCADE,
  run_id INTEGER REFERENCES understand_run(id),
  created_at INTEGER NOT NULL
);
CREATE INDEX matter_add_matter ON matter_add(matter_id);

DROP TRIGGER IF EXISTS matter_insert;
DROP TRIGGER IF EXISTS matter_update;
DROP TRIGGER IF EXISTS matter_decide;
DROP TRIGGER IF EXISTS matter_item_insert;
DROP TRIGGER IF EXISTS matter_item_update;
DROP TRIGGER IF EXISTS matter_delete;

-- Rule 1: a proposal can be deleted as a row, and a confirmed matter with
-- fewer than 2 member rows (the delete of a matter by hand with one item:
-- a declined document needs 2 items).
CREATE TRIGGER matter_delete BEFORE DELETE ON matter
WHEN old.status = 'declined'
  OR (old.status = 'confirmed'
      AND (SELECT count(*) FROM matter_item WHERE matter_id = old.id) >= 2)
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid delete of a matter');
END;

-- Rule 1: a matter starts as an unsealed proposal. A matter of the model
-- names its model and prompt; a matter of an agent or the person has
-- none ('').
CREATE TRIGGER matter_insert BEFORE INSERT ON matter
WHEN new.status IS NOT 'proposed' OR new.sealed IS NOT 0 OR new.keep IS NOT NULL
  -- `INSERT OR REPLACE` must not replace a matter that exists.
  OR EXISTS (SELECT 1 FROM matter WHERE id = new.id)
  OR (new.author = 'model' AND (new.model = '' OR new.prompt = ''))
  OR (new.author IS NOT 'model' AND (new.model IS NOT '' OR new.prompt IS NOT ''))
BEGIN
  SELECT RAISE(ABORT, 'iris: a matter starts as a proposal of its author');
END;

-- Rules 1 and 3: a proposal changes as in migration 16. A confirmed matter
-- changes its title and `updated_at` only, or moves to `declined` (the
-- delete) with `keep` set to NULL. A declined matter does not change.
CREATE TRIGGER matter_update BEFORE UPDATE ON matter
WHEN new.id IS NOT old.id OR new.author IS NOT old.author
  OR new.model IS NOT old.model OR new.prompt IS NOT old.prompt
  OR new.run_id IS NOT old.run_id OR new.created_at IS NOT old.created_at
  OR old.status = 'declined'
  OR (old.status = 'proposed' AND (
    -- The seal goes from 0 to 1 only, and a decision needs the seal.
    new.sealed < old.sealed
    OR (new.status IS NOT 'proposed' AND new.sealed IS NOT 1)))
  OR (old.status = 'confirmed' AND (
    new.sealed IS NOT old.sealed
    OR new.status NOT IN ('confirmed', 'declined')
    OR (new.status = 'confirmed' AND (new.keep IS NOT old.keep
                                      OR new.decided_at IS NOT old.decided_at))
    -- The delete sets its own time as the decision and the last change.
    -- A declined matter needs 2 member rows: one with fewer goes as a row.
    OR (new.status = 'declined' AND (new.decided_at IS NOT new.updated_at
                                     OR (SELECT count(*) FROM matter_item
                                         WHERE matter_id = old.id) < 2
                                     OR new.updated_at < old.updated_at
                                     OR new.decided_at IS old.decided_at))))
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid change of a matter');
END;

-- Rule 2: a decision on a proposal of the model or an agent needs a valid
-- proposal (the view `matter_valid`). A confirm keeps 1 or more distinct
-- live members, by their item row IDs; a matter by hand (`person`) needs
-- no `matter_valid`. The CASE stops `json_each` on text that is not JSON.
CREATE TRIGGER matter_decide BEFORE UPDATE OF status ON matter
WHEN old.status = 'proposed' AND new.status IS NOT 'proposed' AND (
  (old.author IS NOT 'person'
   AND NOT EXISTS (SELECT 1 FROM matter_valid WHERE matter_id = old.id))
  OR (old.author = 'person' AND new.status IS NOT 'confirmed')
  OR (new.status = 'confirmed' AND NOT CASE
    WHEN json_valid(new.keep) AND json_type(new.keep) = 'array' THEN
      json_array_length(new.keep) BETWEEN 1 AND 50
      AND (SELECT count(DISTINCT value) FROM json_each(new.keep))
          = json_array_length(new.keep)
      AND NOT EXISTS (
        SELECT 1 FROM json_each(new.keep)
        WHERE type IS NOT 'integer' OR value NOT IN (
          SELECT item_id FROM matter_live WHERE matter_id = old.id))
    ELSE 0 END))
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid decision');
END;

-- Rule 9: a matter that leaves `confirmed` has no pending addition.
CREATE TRIGGER matter_add_end AFTER UPDATE OF status ON matter
WHEN new.status IS NOT 'confirmed'
BEGIN
  DELETE FROM matter_add WHERE matter_id = new.id;
END;

-- Rules 4 to 7 on a new member row: a proposal gets kept members while it
-- is unsealed; a confirmed matter gets an add (`kept` = 1) or a refusal
-- (`kept` = 0). A kept member is in one open matter at most.
CREATE TRIGGER matter_item_insert BEFORE INSERT ON matter_item
WHEN new.kept NOT IN (0, 1)
  OR NOT coalesce((SELECT (status = 'proposed' AND sealed = 0 AND new.kept = 1)
                          OR status = 'confirmed'
                   FROM matter WHERE id = new.matter_id), 0)
  OR (SELECT form FROM item WHERE id = new.item_id)
     NOT IN ('commitment', 'conversation', 'record', 'signal')
  OR (new.kept = 1 AND EXISTS (
    SELECT 1 FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
    WHERE mi.item_id = new.item_id AND mi.kept = 1
      AND m.status IN ('proposed', 'confirmed')))
  -- No matter has more than 50 kept members: a proposal or a confirmed one.
  OR (new.kept = 1 AND (SELECT count(*) FROM matter_item
                        WHERE matter_id = new.matter_id AND kept = 1) >= 50)
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid member');
END;

-- Rule 5: only `kept` changes, and only in a confirmed matter. A change to
-- 1 keeps rule 6 and the limit of 50; a change to 0 leaves 1 or more kept
-- members.
CREATE TRIGGER matter_item_update BEFORE UPDATE ON matter_item
WHEN new.matter_id IS NOT old.matter_id OR new.item_id IS NOT old.item_id
  OR (new.kept IS NOT old.kept AND (
    (SELECT status FROM matter WHERE id = old.matter_id) IS NOT 'confirmed'
    OR new.kept NOT IN (0, 1)
    OR (new.kept = 1 AND (
      EXISTS (SELECT 1 FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
              WHERE mi.item_id = old.item_id AND mi.kept = 1
                AND m.status IN ('proposed', 'confirmed'))
      OR (SELECT count(*) FROM matter_item
          WHERE matter_id = old.matter_id AND kept = 1) >= 50))
    OR (new.kept = 0 AND NOT EXISTS (
      SELECT 1 FROM matter_item WHERE matter_id = old.matter_id AND kept = 1
        AND item_id IS NOT old.item_id))))
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid change of a member');
END;

-- Rule 9: an item that becomes a kept member has no pending addition, and
-- a member row (an add or a refusal) ends the addition of the same matter.
CREATE TRIGGER matter_item_kept_add AFTER INSERT ON matter_item
BEGIN
  DELETE FROM matter_add
  WHERE item_id = new.item_id AND (new.kept = 1 OR matter_id = new.matter_id);
END;

CREATE TRIGGER matter_item_kept_update AFTER UPDATE OF kept ON matter_item
WHEN new.kept = 1
BEGIN
  DELETE FROM matter_add WHERE item_id = new.item_id;
END;

-- Rule 9: an addition goes to a confirmed matter, for an item of a member
-- form with no member row in that matter and no kept row of an open
-- matter. An addition does not change.
CREATE TRIGGER matter_add_insert BEFORE INSERT ON matter_add
WHEN (SELECT status FROM matter WHERE id = new.matter_id) IS NOT 'confirmed'
  OR (SELECT form FROM item WHERE id = new.item_id)
     NOT IN ('commitment', 'conversation', 'record', 'signal')
  OR EXISTS (SELECT 1 FROM matter_item
             WHERE matter_id = new.matter_id AND item_id = new.item_id)
  OR EXISTS (SELECT 1 FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
             WHERE mi.item_id = new.item_id AND mi.kept = 1
               AND m.status IN ('proposed', 'confirmed'))
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid addition');
END;

CREATE TRIGGER matter_add_update BEFORE UPDATE ON matter_add
BEGIN
  SELECT RAISE(ABORT, 'iris: an addition does not change');
END;

-- Signals are members (D21): `matter_live` of migration 44 with the form
-- `signal`. `matter_add_live` has the same eligibility, without `kept`.
DROP VIEW matter_live;

CREATE VIEW matter_live AS
  SELECT mi.matter_id, mi.item_id, i.form, msg.thread_id
  FROM matter_item mi
  JOIN item i ON i.id = mi.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE mi.kept = 1
    AND i.form IN ('commitment', 'conversation', 'record', 'signal')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND (i.form <> 'signal' OR i.status = 'active')
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'
    AND NOT (msg.hostile IS NOT NULL AND msg.hostile NOT IN ('', '[]'))
    AND NOT EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
                    WHERE hml.message_id = msg.id AND hl.provider_id = 'SPAM')
    AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender hs
                      ON hs.address = hh.from_addr COLLATE NOCASE
                    WHERE hh.message_id = msg.id AND hs.kind = 'hostile');

-- The additions that the person can decide: the item is live and the
-- matter is confirmed.
CREATE VIEW matter_add_live AS
  SELECT a.matter_id, a.item_id, a.created_at, i.form
  FROM matter_add a
  JOIN matter m ON m.id = a.matter_id AND m.status = 'confirmed'
  JOIN item i ON i.id = a.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE i.form IN ('commitment', 'conversation', 'record', 'signal')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND (i.form <> 'signal' OR i.status = 'active')
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'
    AND NOT (msg.hostile IS NOT NULL AND msg.hostile NOT IN ('', '[]'))
    AND NOT EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
                    WHERE hml.message_id = msg.id AND hl.provider_id = 'SPAM')
    AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender hs
                      ON hs.address = hh.from_addr COLLATE NOCASE
                    WHERE hh.message_id = msg.id AND hs.kind = 'hostile');
