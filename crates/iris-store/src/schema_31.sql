-- Migration 31: version 1 of the open model (docs/specs/2026-11-17-open-model-v1.md,
-- card R6-07). Each stored item document gets the `model` value `iris/1.0`.
-- Only `item.doc` stores a document with a `model` value. The other
-- documents are written when they are read.
--
-- The update changes only `$.model`. The trigger `attention_due_item` of
-- migration 8 still queues each item with no `attention` field when its
-- `doc` changes. So the migration keeps the queue as it was: it saves the
-- queue before the update and removes each row that the update added.
-- `attention_keep` acts only when `attention` goes away, so it does nothing.

CREATE TEMP TABLE model_v1_due AS SELECT message_id FROM attention_due;

UPDATE item SET doc = json_set(doc, '$.model', 'iris/1.0')
  WHERE json_extract(doc, '$.model') = 'iris/0';

DELETE FROM attention_due
  WHERE message_id NOT IN (SELECT message_id FROM model_v1_due);

DROP TABLE model_v1_due;
