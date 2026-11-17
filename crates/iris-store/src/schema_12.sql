-- Migration 12: the data of the measures page (card R5-12, the manifesto
-- "How we measure Iris") that the store does not have yet.
--
-- One row for each day on which the person used the web client, by the
-- local date of the person. The rows stay on this machine.
CREATE TABLE use_day (
  day TEXT PRIMARY KEY
    CHECK (day GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(day) IS day)
) WITHOUT ROWID;

-- One row for each source mail that the person opened on a day of use. A
-- second open of the same mail on the same day adds no row, so a retry of
-- an open with a lost answer does not count twice. No foreign key to
-- `message`: a message that goes stays an open of its day.
CREATE TABLE use_open (
  day TEXT NOT NULL REFERENCES use_day(day),
  message_id INTEGER NOT NULL,
  PRIMARY KEY (day, message_id)
) WITHOUT ROWID;

-- The two times of the measure "time from install to the first brief"
-- (specification 1, M4): `connect`, when the first account is added (the
-- connect step), and `brief`, when the brief first has content
-- (iris_api::measures::check_brief, every 10 seconds in iris serve). Each
-- time is written once. A time never changes after that.
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

CREATE TRIGGER measure_time_update BEFORE UPDATE ON measure_time
BEGIN
  SELECT RAISE(ABORT, 'iris: a measure time does not change');
END;

-- A store from before this migration: the best evidence is the start of
-- the oldest sync and the oldest item that is still there (an estimate:
-- the brief can show its first content later). An account with no sync
-- yet has only the time of this migration.
INSERT INTO measure_time(name, at)
SELECT 'connect', coalesce(min(started_at), CAST(unixepoch('subsec') * 1000 AS INTEGER))
FROM sync_log HAVING count(*) > 0 OR EXISTS (SELECT 1 FROM account);
INSERT INTO measure_time(name, at)
SELECT 'brief', min(created_at) FROM item HAVING count(*) > 0;

-- The decision of the person on each action of another author (an agent,
-- triage or a local tool): the measure "proposed actions that you approve
-- without an edit". A later state (applied, expired, cancelled) changes
-- `decided_by` of the action, so the decision has its own row. A new
-- decision of the person on the same action replaces the row.
CREATE TABLE person_decision (
  action_id INTEGER PRIMARY KEY REFERENCES changeset_action(id),
  approved INTEGER NOT NULL CHECK (approved IN (0, 1)),
  at INTEGER NOT NULL
);

CREATE TRIGGER person_decision AFTER UPDATE OF state ON changeset_action
WHEN new.decided_by IS 'person' AND new.state IN ('approved', 'denied')
  AND new.state IS NOT old.state
  AND (SELECT author_kind FROM changeset WHERE id = new.changeset_id) <> 'person'
BEGIN
  DELETE FROM person_decision WHERE action_id = new.id;
  INSERT INTO person_decision(action_id, approved, at)
  VALUES (new.id, new.state = 'approved', coalesce(new.decided_at, 0));
END;

-- The decisions of the person that the actions still show.
INSERT INTO person_decision(action_id, approved, at)
SELECT a.id, a.state <> 'denied', coalesce(a.decided_at, 0)
FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
WHERE a.decided_by = 'person' AND c.author_kind <> 'person';
