-- Migration 13: the stranger gate (card R5-03, specification 1, section 6.7).
--
-- `declined_at` is the time when the person declined the sender at the gate,
-- or NULL. It is a decision of the person apart from the kind: the engine
-- keeps the kind current, so a declined sender can still become `hostile`.
-- The decline applies only while the kind is `stranger`.

ALTER TABLE sender ADD COLUMN declined_at INTEGER;
