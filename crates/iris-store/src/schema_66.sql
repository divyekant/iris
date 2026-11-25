-- Migration 66 (issue 479): a reply draft keeps the set of the messages
-- of its thread that a reply can answer, as the model read it: their row
-- IDs in order, as text (`seen`, for example "4,7,9"). A change of the set
-- after the save makes the draft not ready (`iris_voice::ready_sql`): a
-- late sync of an older message, a message that leaves the trash or spam,
-- a message that moves into the thread, and also one that leaves the set.
-- A row of an older build has no value: NULL, and only the rules of
-- `based_on` apply to it (limit issue #1107).
ALTER TABLE reply_draft ADD COLUMN seen TEXT;
