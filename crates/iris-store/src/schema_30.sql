-- Migration 30: `rule_proposal` also holds the `hide` proposals from the
-- choices of the person (docs/specs/2026-11-17-preference-rules.md, card
-- R5-02).
--
-- 1. `verb` accepts `hide`: for a `hide` row, the verb is the effect of the
--    rule, and its condition is one `sender`.
-- 2. The new column `form` is `feed`, `conversation` or `signal` for a
--    `hide` row, and `''` for an `allow` row. It is never NULL, because two
--    NULL values are different in a unique key.
-- 3. The unique key is `(verb, cond, value, form)`.
--
-- SQLite cannot change a CHECK or a key in place, so the migration makes a
-- new table, copies each row with its ID, drops the old table and renames
-- the new one. No ID is used again: the `seq` of the old table in
-- `sqlite_sequence` (the highest ID ever given, also of a deleted row) goes
-- to the new table.

CREATE TABLE rule_proposal_new (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  verb TEXT NOT NULL CHECK (verb IN ('archive', 'inbox', 'read', 'unread', 'trash', 'hide')),
  -- The one condition of the rule (with `form` for `hide`), and its value.
  cond TEXT NOT NULL CHECK (cond IN ('sender', 'sender_kind')),
  value TEXT NOT NULL,
  form TEXT NOT NULL DEFAULT '' CHECK (
    (verb = 'hide' AND cond = 'sender' AND form IN ('feed', 'conversation', 'signal'))
    OR (verb <> 'hide' AND form = '')),
  -- The number of approvals (or choices, for `hide`) that the rule would
  -- have decided.
  approvals INTEGER NOT NULL CHECK (approvals >= 1),
  state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'accepted', 'dismissed')),
  -- The ID of the rule in the file, after the accept.
  rule INTEGER CHECK ((state = 'accepted') = (rule IS NOT NULL)),
  -- Seconds since 1970-01-01 UTC.
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (verb, cond, value, form)
);

INSERT INTO rule_proposal_new
  (id, verb, cond, value, form, approvals, state, rule, created_at, updated_at)
SELECT id, verb, cond, value, '', approvals, state, rule, created_at, updated_at
FROM rule_proposal;

CREATE TEMP TABLE migration_30 (seq INTEGER NOT NULL);
INSERT INTO migration_30
SELECT coalesce((SELECT seq FROM sqlite_sequence WHERE name = 'rule_proposal'), 0);

-- The drop also drops the two triggers of migration 15.
DROP TABLE rule_proposal;
ALTER TABLE rule_proposal_new RENAME TO rule_proposal;

-- The copy wrote the highest copied ID as the `seq`. The old `seq` can be
-- larger, after a delete of the newest row.
DELETE FROM sqlite_sequence WHERE name = 'rule_proposal';
INSERT INTO sqlite_sequence(name, seq)
SELECT 'rule_proposal', max((SELECT seq FROM migration_30),
                            coalesce((SELECT max(id) FROM rule_proposal), 0));
DROP TABLE migration_30;

-- The person decides a proposal one time (as migration 15).
CREATE TRIGGER rule_proposal_state BEFORE UPDATE OF state ON rule_proposal
WHEN old.state <> 'open' AND new.state IS NOT old.state
BEGIN
  SELECT RAISE(ABORT, 'iris: a decided proposal does not change');
END;

CREATE TRIGGER rule_proposal_delete BEFORE DELETE ON rule_proposal
WHEN old.state <> 'open'
BEGIN
  SELECT RAISE(ABORT, 'iris: a decided proposal does not change');
END;
