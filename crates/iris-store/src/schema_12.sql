-- Migration 12: the data of the measures page (card R5-12, the manifesto
-- "How we measure Iris") that the store does not have yet.
--
-- One row for each day on which the person used the web client, by the
-- local date of the person. `opens` counts the source mail that the person
-- opened on that day. The rows stay on this machine.

CREATE TABLE use_day (
  day TEXT PRIMARY KEY
    CHECK (day GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(day) IS day),
  opens INTEGER NOT NULL DEFAULT 0 CHECK (opens >= 0)
) WITHOUT ROWID;

-- The two times of the measure "time from install to the first brief"
-- (specification 1, M4): `connect`, when the first account is added (the
-- connect step), and `brief`, when the first item is made (the first thing
-- that the brief can show). The triggers write each time once. A time
-- never changes after that.
CREATE TABLE measure_time (
  name TEXT PRIMARY KEY CHECK (name IN ('connect', 'brief')),
  at INTEGER NOT NULL
) WITHOUT ROWID;

-- NOT EXISTS, not OR IGNORE: an UPSERT of the outer statement overrides the
-- conflict clause of a trigger.
CREATE TRIGGER measure_connect AFTER INSERT ON account
WHEN NOT EXISTS (SELECT 1 FROM measure_time WHERE name = 'connect')
BEGIN
  INSERT INTO measure_time(name, at)
  SELECT 'connect', CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE NOT EXISTS (SELECT 1 FROM measure_time WHERE name = 'connect');
END;

CREATE TRIGGER measure_brief AFTER INSERT ON item
WHEN NOT EXISTS (SELECT 1 FROM measure_time WHERE name = 'brief')
BEGIN
  INSERT INTO measure_time(name, at)
  SELECT 'brief', CAST(unixepoch('subsec') * 1000 AS INTEGER)
  WHERE NOT EXISTS (SELECT 1 FROM measure_time WHERE name = 'brief');
END;

CREATE TRIGGER measure_time_update BEFORE UPDATE ON measure_time
BEGIN
  SELECT RAISE(ABORT, 'iris: a measure time does not change');
END;

-- A store from before this migration: the best evidence is the start of
-- the oldest sync and the oldest item that is still there.
INSERT INTO measure_time(name, at)
SELECT 'connect', min(started_at) FROM sync_log HAVING count(*) > 0;
INSERT INTO measure_time(name, at)
SELECT 'brief', min(created_at) FROM item HAVING count(*) > 0;
