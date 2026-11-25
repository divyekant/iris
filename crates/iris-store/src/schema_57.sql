-- Issue 478: the summary of a long thread ("Summary ready",
-- docs/specs/2026-11-25-thread-summary.md). One summary for each thread: a
-- new summary replaces the old one. The summary is ready only while
-- `based_on` is the newest answerable message of the thread, no message of
-- the thread is hostile or waits for understanding (the ready rule of a
-- reply draft), and `basis` is the basis of the thread now: each message of
-- the snapshot as "id:content_sha256". So a change of the mail that the
-- summary read makes it stale with no write.

CREATE TABLE IF NOT EXISTS thread_summary (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  -- The newest answerable message that the summary covers.
  based_on INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  -- Each message of the snapshot, newest first, as "id:content_sha256",
  -- joined with "," (iris_summary::basis_sql).
  basis TEXT NOT NULL,
  text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 1500),
  model TEXT NOT NULL,
  prompt TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

-- The last try of each thread. `invalid` = 1: the model gave no valid
-- summary twice, so the same basis and model version get no call.
-- The order of the candidates reads `at`: a thread that the budget cannot
-- pay moves to the end.
CREATE TABLE IF NOT EXISTS thread_summary_try (
  thread_id INTEGER PRIMARY KEY REFERENCES thread(id) ON DELETE CASCADE,
  based_on INTEGER NOT NULL,
  basis TEXT NOT NULL,
  model_version INTEGER NOT NULL,
  invalid INTEGER NOT NULL CHECK (invalid IN (0, 1)),
  at INTEGER NOT NULL
);
