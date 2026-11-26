-- Migration 73 (issue 1168): before this change, the cleanup of gone mail
-- deleted the message row of mail that the model did not read and left
-- its notice open when no model was set. After a model is set, such a
-- notice showed the sender and the subject that no model read. The
-- cleanup now marks done each open notice of mail that the model did not
-- read (`model_read!`). This marks done each open rule notice whose message
-- row is gone, because no row tells if the model read it. The mail went
-- away 30 days or more before, so the notice is old.
UPDATE notice SET done_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000
 WHERE done_at IS NULL AND message_id IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM message m WHERE m.id = notice.message_id);
