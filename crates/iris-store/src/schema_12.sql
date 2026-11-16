-- Migration 12: the reply drafts of card R3-05 (crate iris-voice).
-- One draft for each thread: a new draft replaces the old one. The draft
-- stays in the store. Nothing sends it or copies it to Gmail.

CREATE TABLE reply_draft (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  -- The newest message of the thread when the model wrote the draft. If the
  -- thread has a newer message now, the draft does not know it.
  based_on INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  intent TEXT,
  body TEXT NOT NULL,
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  samples INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
