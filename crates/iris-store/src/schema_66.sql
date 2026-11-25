-- Migration 66 (issue 479): a reply draft keeps the set of the messages
-- of its thread that a reply can answer at the save, as their count
-- (`seen`) and the sum of their row IDs (`seen_sum`). A change of the set
-- after the save makes the draft not ready (`iris_voice::ready_sql`): a
-- late sync of an older message, a message that leaves the trash or spam,
-- a message that moves into the thread, and also one that leaves the set.
-- A row of an older build has no values: NULL, and only the rules of
-- `based_on` apply to it (limit issue of the PR).
ALTER TABLE reply_draft ADD COLUMN seen INTEGER;
ALTER TABLE reply_draft ADD COLUMN seen_sum INTEGER;
