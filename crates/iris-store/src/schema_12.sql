-- Migration 12: watches (card R5-07, docs/specs/2026-11-16-watch.md).
--
-- A watch is a search text (specification 1, section 5.6). It gives one
-- notice: the first new message that matches. Then it waits for nothing.

CREATE TABLE watch (
  -- AUTOINCREMENT: the engine never uses an ID again (specification 1, 5.7).
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  text TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  -- The largest message row ID at the start. Only a larger row ID is new.
  after_message INTEGER NOT NULL,
  -- The first message that matched, and when the engine found it. Both
  -- are NULL while the watch waits. No foreign key: the cleanup of a gone
  -- message must not delete or reset the notice, and AUTOINCREMENT never
  -- gives the row ID of a deleted message to a new message.
  message_id INTEGER,
  arrived_at INTEGER,
  CHECK ((message_id IS NULL) = (arrived_at IS NULL))
);

-- The check of the engine reads only the watches that wait.
CREATE INDEX watch_waits ON watch(id) WHERE message_id IS NULL;
