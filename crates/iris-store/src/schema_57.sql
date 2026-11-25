-- Migration 57 (issue 972): a signal past its `expires` time is not live,
-- also while its stored status is still `active`. The views `matter_live`
-- and `matter_add_live` of migration 46 read the status only, and the
-- engine moves the status to `expired` one tick later. These views also
-- compare `expires` with the clock, so each reader (the matter views, the
-- valid rule of a proposal, the decide trigger) agrees at once. The rule
-- `live_item!` of iris-matters has the same condition.

DROP VIEW IF EXISTS matter_live;

CREATE VIEW matter_live AS
  SELECT mi.matter_id, mi.item_id, i.form, msg.thread_id
  FROM matter_item mi
  JOIN item i ON i.id = mi.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE mi.kept = 1
    AND i.form IN ('commitment', 'conversation', 'record', 'signal')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND (i.form <> 'signal' OR i.status = 'active')
    AND (i.form <> 'signal'
         OR coalesce(unixepoch(json_extract(i.doc, '$.expires'), 'subsec')
                     > unixepoch('now', 'subsec'), 1))
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'
    AND NOT (msg.hostile IS NOT NULL AND msg.hostile NOT IN ('', '[]'))
    AND NOT EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
                    WHERE hml.message_id = msg.id AND hl.provider_id = 'SPAM')
    AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender hs
                      ON hs.address = hh.from_addr COLLATE NOCASE
                    WHERE hh.message_id = msg.id AND hs.kind = 'hostile');

-- The additions that the person can decide: the item is live and the
-- matter is confirmed.
DROP VIEW IF EXISTS matter_add_live;

CREATE VIEW matter_add_live AS
  SELECT a.matter_id, a.item_id, a.created_at, i.form
  FROM matter_add a
  JOIN matter m ON m.id = a.matter_id AND m.status = 'confirmed'
  JOIN item i ON i.id = a.item_id
  JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
  JOIN message msg ON msg.id = s.message_id
  WHERE i.form IN ('commitment', 'conversation', 'record', 'signal')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND (i.form <> 'signal' OR i.status = 'active')
    AND (i.form <> 'signal'
         OR coalesce(unixepoch(json_extract(i.doc, '$.expires'), 'subsec')
                     > unixepoch('now', 'subsec'), 1))
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'
    AND NOT (msg.hostile IS NOT NULL AND msg.hostile NOT IN ('', '[]'))
    AND NOT EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
                    WHERE hml.message_id = msg.id AND hl.provider_id = 'SPAM')
    AND NOT EXISTS (SELECT 1 FROM header hh JOIN sender hs
                      ON hs.address = hh.from_addr COLLATE NOCASE
                    WHERE hh.message_id = msg.id AND hs.kind = 'hostile');
