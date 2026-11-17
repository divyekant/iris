-- Migration 32: the nudges of card R3-07. First, the commitment of a nudge. A nudge is a reply
-- draft with the fixed nudge intent of iris-voice. `ask` is the commitment
-- that the model got (title, due and quote, as JSON text), so a nudge is
-- ready only while its commitment is the same. NULL for other drafts.

ALTER TABLE reply_draft ADD COLUMN ask TEXT;

-- The last nudge try of each thread, so the engine keeps no list in
-- memory. `blocked` is 1 after the model gave an answer that is not a
-- valid draft: no new call for the same key and model version. Each try
-- moves the thread to the end of the order of the next tries.
CREATE TABLE nudge_try (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  version INTEGER NOT NULL,
  blocked INTEGER NOT NULL CHECK (blocked IN (0, 1)),
  tried_at INTEGER NOT NULL
);
