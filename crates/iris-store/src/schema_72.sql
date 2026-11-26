-- Migration 72 (issue 1121): each live message gets one more link check,
-- for the links of attached messages (issue 372, PR 1122). The check of
-- mail from before that change did not read them.
--
-- `link_recheck` is not `link_check`: a message in `link_check` may be
-- hostile while it waits (`may_be_hostile`), so it leaves the context of
-- the later messages and changes the attention of its items. The mail of
-- this queue had its check, so it stays as it is while it waits. The
-- understanding step (`iris_understand::store::recheck_links_scan`) reads
-- each row once. Only a message with a deceptive link moves to
-- `link_check`, and the normal check gives it the mark.
CREATE TABLE IF NOT EXISTS link_recheck (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE
);

INSERT INTO link_recheck(message_id)
  SELECT m.id FROM message m
   WHERE m.gone_at IS NULL AND m.content_sha256 IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM link_check l WHERE l.message_id = m.id);
