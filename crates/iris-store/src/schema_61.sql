-- Migration 61: a notice row when a job needs you (issues #844 and #845,
-- docs/specs/2026-11-25-job-notices.md).
--
-- The table `notice` gets job rows: one for each move of a job to
-- `needs_you`, by a trigger in the transaction of the move. A rule row has
-- a mail and a rule; a job row has a job event and no rule.

-- A test that sets the schema version back runs this migration again on a
-- store that has the trigger: the rename below then fails on it.
DROP TRIGGER IF EXISTS notice_job;

CREATE TABLE notice_new (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- No foreign key: the cleanup of gone mail keeps the notice.
  message_id INTEGER UNIQUE,
  -- The job and the event of its move to `needs_you`. No foreign key, as
  -- for the mail.
  job_id INTEGER,
  job_event_id INTEGER UNIQUE,
  -- The rule and the active policy version that matched (a rule row).
  rule INTEGER,
  -- The name of the rule at the time of the notice, or `Job due` or
  -- `Job question` for a job row.
  rule_name TEXT NOT NULL,
  policy_version INTEGER,
  -- A rule row: the sender and the subject. A job row: `j-<n>` and the
  -- goal of the person.
  title TEXT NOT NULL,
  text TEXT NOT NULL,
  -- Milliseconds since 1970-01-01 UTC.
  created_at INTEGER NOT NULL,
  done_at INTEGER,
  CHECK ((message_id IS NULL) <> (job_event_id IS NULL)),
  CHECK ((job_id IS NULL) = (job_event_id IS NULL)),
  CHECK ((message_id IS NULL) = (rule IS NULL)),
  CHECK ((message_id IS NULL) = (policy_version IS NULL))
);

INSERT INTO notice_new (id, message_id, rule, rule_name, policy_version, title, text, created_at, done_at)
  SELECT id, message_id, rule, rule_name, policy_version, title, text, created_at, done_at FROM notice;

-- Keep the sequence of the old table when its largest row is gone.
UPDATE sqlite_sequence
  SET seq = max(seq, (SELECT seq FROM sqlite_sequence WHERE name = 'notice'))
  WHERE name = 'notice_new' AND EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'notice');
INSERT INTO sqlite_sequence (name, seq)
  SELECT 'notice_new', seq FROM sqlite_sequence
  WHERE name = 'notice' AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'notice_new');

DROP TABLE notice;
ALTER TABLE notice_new RENAME TO notice;

-- Each move to `needs_you` adds a job row. `add_event` is the one insert of
-- `job_event`, and each move goes through it, so this covers each path.
CREATE TRIGGER notice_job AFTER INSERT ON job_event
WHEN NEW.state = 'needs_you'
BEGIN
  INSERT INTO notice (job_id, job_event_id, rule_name, title, text, created_at)
  SELECT NEW.job_id, NEW.id,
         CASE NEW.by WHEN 'engine' THEN 'Job due' ELSE 'Job question' END,
         'j-' || NEW.job_id,
         substr(j.goal, 1, 2000),
         NEW.at
  FROM job j WHERE j.id = NEW.job_id;
END;
