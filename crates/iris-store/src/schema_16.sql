-- Migration 16: notices by rule (card R5-08,
-- docs/specs/2026-11-16-notices.md).
--
-- One row for each new mail that a `notify` rule matched. The words stay in
-- the row, so the notice still reads after the mail is gone.

CREATE TABLE notice (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  -- No foreign key: the cleanup of gone mail keeps the notice.
  message_id INTEGER NOT NULL UNIQUE,
  -- The rule and the active policy version that matched.
  rule INTEGER NOT NULL,
  -- The name of the rule at the time of the notice.
  rule_name TEXT NOT NULL,
  policy_version INTEGER NOT NULL,
  -- The sender and the subject, one line each.
  title TEXT NOT NULL,
  text TEXT NOT NULL,
  -- Milliseconds since 1970-01-01 UTC.
  created_at INTEGER NOT NULL,
  done_at INTEGER
);

-- The pass reads the mail of the last hour.
CREATE INDEX message_fetched ON message(fetched_at);
