-- Issue 311: the job page finds the changesets of one job with
-- `WHERE author_job = ?`. This index serves that lookup, so the page does
-- not read each changeset row.
-- IF NOT EXISTS: the tests that set an older user_version run it again.
CREATE INDEX IF NOT EXISTS changeset_author_job ON changeset(author_job);
