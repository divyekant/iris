-- Migration 66 (issue 479): a reply draft keeps the highest row ID of
-- the messages of its thread at the save (`seen`). A message that a reply
-- can answer and that comes into the thread after the draft, also one
-- with an older date (a late sync), has a higher row ID, so the draft is
-- not ready (`iris_voice::ready_sql`). A row of an older build has no
-- value: NULL, and only the rules of `based_on` apply to it.
ALTER TABLE reply_draft ADD COLUMN seen INTEGER;
