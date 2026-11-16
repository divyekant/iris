-- Migration 13: matters (card R6-01, docs/specs/2026-11-16-matters.md).
--
-- A matter is a title and the items that belong together. Iris proposes a
-- matter; the person confirms or declines it. The triggers below hold the
-- rules of the spec ("Store: migration 12" there; this is the 13th
-- migration), so no write path can break them.

CREATE TABLE matter (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 80),
  status TEXT NOT NULL CHECK (status IN ('proposed', 'confirmed', 'declined')),
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  run_id INTEGER REFERENCES understand_run(id),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  decided_at INTEGER,
  CHECK ((status = 'proposed') = (decided_at IS NULL))
);

CREATE TABLE matter_item (
  matter_id INTEGER NOT NULL REFERENCES matter(id) ON DELETE CASCADE,
  item_id INTEGER NOT NULL REFERENCES item(id) ON DELETE CASCADE,
  kept INTEGER NOT NULL DEFAULT 1 CHECK (kept IN (0, 1)),
  PRIMARY KEY (matter_id, item_id)
) WITHOUT ROWID;
CREATE INDEX matter_item_item ON matter_item(item_id);

-- The last proposal call: `tried_at` before each call, `candidates` (the
-- fingerprint of the sent set) after an answer.
CREATE TABLE matter_run (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  tried_at INTEGER,
  candidates TEXT
);

-- Rule 1: a matter starts as a proposal, moves only from `proposed` to
-- `confirmed` or `declined`, and changes no more after the decision. Only a
-- proposal can be deleted.
CREATE TRIGGER matter_insert BEFORE INSERT ON matter
WHEN new.status IS NOT 'proposed'
BEGIN
  SELECT RAISE(ABORT, 'iris: a matter starts as a proposal');
END;

CREATE TRIGGER matter_update BEFORE UPDATE ON matter
WHEN old.status IS NOT 'proposed'
  OR new.id IS NOT old.id OR new.model IS NOT old.model
  OR new.prompt IS NOT old.prompt OR new.run_id IS NOT old.run_id
  OR new.created_at IS NOT old.created_at
BEGIN
  SELECT RAISE(ABORT, 'iris: a decided matter does not change');
END;

CREATE TRIGGER matter_delete BEFORE DELETE ON matter
WHEN old.status IS NOT 'proposed'
BEGIN
  SELECT RAISE(ABORT, 'iris: only a proposal can be deleted');
END;

-- Rules 2, 3 and 4 on a new member row. The members go in with the
-- proposal: only the newest matter, while it is a proposal, gets rows.
CREATE TRIGGER matter_item_insert BEFORE INSERT ON matter_item
WHEN new.kept IS NOT 1
  OR new.matter_id IS NOT (SELECT max(id) FROM matter)
  OR (SELECT status FROM matter WHERE id = new.matter_id) IS NOT 'proposed'
  OR (SELECT form FROM item WHERE id = new.item_id)
     NOT IN ('commitment', 'conversation', 'record')
  OR EXISTS (
    SELECT 1 FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
    WHERE mi.item_id = new.item_id AND mi.kept = 1
      AND m.status IN ('proposed', 'confirmed'))
BEGIN
  SELECT RAISE(ABORT, 'iris: not a valid member of a new proposal');
END;

-- Rule 3: only `kept` changes, from 1 to 0, while the matter is a proposal
-- (the confirm writes it before the status).
CREATE TRIGGER matter_item_update BEFORE UPDATE ON matter_item
WHEN new.matter_id IS NOT old.matter_id OR new.item_id IS NOT old.item_id
  OR NOT (new.kept IS old.kept OR (old.kept = 1 AND new.kept = 0))
  OR (SELECT status FROM matter WHERE id = old.matter_id) IS NOT 'proposed'
BEGIN
  SELECT RAISE(ABORT, 'iris: a member row does not change');
END;

-- Rule 3: a member row goes only with its item or its matter (the two
-- cascades). A cascade runs after the delete of the parent row, so a
-- direct delete is the one case where both parent rows still exist.
CREATE TRIGGER matter_item_delete BEFORE DELETE ON matter_item
WHEN EXISTS (SELECT 1 FROM matter WHERE id = old.matter_id)
  AND EXISTS (SELECT 1 FROM item WHERE id = old.item_id)
BEGIN
  SELECT RAISE(ABORT, 'iris: a member row goes only with its item or its matter');
END;

-- A live member: a kept member whose item is still eligible (the spec,
-- "One rule decides if a proposal is valid"). The thread is the thread of
-- the origin message of the item.
CREATE VIEW matter_live AS
  SELECT mi.matter_id, mi.item_id, i.form, msg.thread_id
  FROM matter_item mi
  JOIN item i ON i.id = mi.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE mi.kept = 1
    AND i.form IN ('commitment', 'conversation', 'record')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden';

-- The one validity rule of a proposal: 2 or more live members from 2 or
-- more threads.
CREATE VIEW matter_valid AS
  SELECT m.id AS matter_id FROM matter m
  WHERE m.status = 'proposed'
    AND (SELECT count(*) FROM matter_live l WHERE l.matter_id = m.id) >= 2
    AND (SELECT count(DISTINCT l.thread_id) FROM matter_live l
         WHERE l.matter_id = m.id) >= 2;

-- A rebuild keeps each kept member of a confirmed matter and each member of
-- a declined matter, with its ID (specification 1, section 7.5).
DROP VIEW item_pin;

CREATE VIEW item_pin AS
  SELECT item_id FROM correction
  UNION
  SELECT ai.item_id FROM action_item ai
  JOIN changeset_action a ON a.id = ai.action_id
  WHERE a.state IN ('proposed', 'in_review', 'approved', 'held')
  UNION
  SELECT ai.item_id FROM action_item ai
  JOIN ledger l ON l.action_id = ai.action_id
  UNION
  SELECT mi.item_id FROM matter_item mi
  JOIN matter m ON m.id = mi.matter_id
  WHERE (m.status = 'confirmed' AND mi.kept = 1) OR m.status = 'declined';
