-- Issue 112: a member whose origin message is hostile mail is not live, so a
-- proposal does not count it, show it or let a confirm keep it. The
-- condition is the same as `iris_store::hostile_mail!()` in src/lib.rs: a
-- hostile mark, the label SPAM, or a hostile sender. The member is live
-- again when the state clears.
DROP VIEW matter_live;

CREATE VIEW matter_live AS
  SELECT mi.matter_id, mi.item_id, i.form, msg.thread_id
  FROM matter_item mi
  JOIN item i ON i.id = mi.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE mi.kept = 1
    AND i.form IN ('commitment', 'conversation', 'record')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'
    AND NOT (msg.hostile IS NOT NULL AND msg.hostile NOT IN ('', '[]'))
    AND NOT EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
                    WHERE hml.message_id = msg.id AND hl.provider_id = 'SPAM')
    AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender hs
                      ON hs.address = hh.from_addr COLLATE NOCASE
                    WHERE hh.message_id = msg.id AND hs.kind = 'hostile');
