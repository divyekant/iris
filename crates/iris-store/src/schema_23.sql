-- Migration 23: the verbs `skip`, `mute` and `dismiss` of the person
-- (docs/specs/2026-11-17-preference-rules.md, card R5-02).
--
-- The three verbs join the verb CHECK of changeset_action, with the method
-- of migration 18: a wider CHECK accepts each stored row, so only the text
-- of the table changes in sqlite_schema, and RESET loads the schema again.

PRAGMA writable_schema = ON;
UPDATE sqlite_schema
SET sql = replace(sql, '''draft'', ''send''))', '''draft'', ''send'', ''skip'', ''mute'', ''dismiss''))')
WHERE type = 'table' AND name = 'changeset_action';
PRAGMA writable_schema = RESET;

-- Stop the migration if the text above did not change: the CHECK of a
-- temporary table refuses 0.
CREATE TEMP TABLE migration_23 (ok INTEGER CHECK (ok = 1));
INSERT INTO migration_23
SELECT instr(sql, '''send'', ''skip'', ''mute'', ''dismiss'')') > 0
FROM sqlite_schema WHERE type = 'table' AND name = 'changeset_action';
DROP TABLE migration_23;

-- A schema change that the other connections see: it changes the schema
-- cookie, as the CREATE TABLE statements of migration 18 do.
CREATE TABLE migration_23_done (id INTEGER PRIMARY KEY);
DROP TABLE migration_23_done;
