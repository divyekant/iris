-- Migration 31: version 1 of the open model (docs/specs/2026-11-17-open-model-v1.md,
-- card R6-07). Each stored item document gets the `model` value `iris/1.0`.
-- Only `item.doc` stores a document with a `model` value. The other
-- documents are written when they are read.
--
-- The update changes only `$.model`, so the attention triggers of migration
-- 8 do nothing: `attention_due_item` reads the form, `type`, `kind` and
-- `attention`, and `attention_keep` acts only when `attention` goes away.

UPDATE item SET doc = json_set(doc, '$.model', 'iris/1.0')
  WHERE json_extract(doc, '$.model') = 'iris/0';
