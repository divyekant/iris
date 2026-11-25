-- Migration 61 (issue 1033): a received message whose header block does
-- not end in the first 256 KiB gets the mark `spoof`. Mail that Iris
-- understood before this rule keeps its old marks, so this queues each
-- understood message that can have such a header (a size of 256 KiB or more, or
-- no size) for the fixed check of `fixed_due` (migration 54). The check
-- runs with no model, and understands a message again only when its marks
-- change.

INSERT INTO fixed_due(message_id)
  SELECT m.id FROM message m
   WHERE m.understood_at IS NOT NULL AND m.gone_at IS NULL
     AND (m.size IS NULL OR m.size >= 262144)
     AND NOT EXISTS (SELECT 1 FROM fixed_due d WHERE d.message_id = m.id);
