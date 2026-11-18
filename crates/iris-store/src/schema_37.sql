-- Migration 37: the category switch of an Outlook account
-- (docs/specs/2026-11-17-outlook-provider.md, assumption A6, card R6-16).
-- The time in milliseconds when Graph accepted a category write but kept
-- another list than the request. From then on, each category write of the
-- account gets a definite refusal. NULL: category writes are on.

ALTER TABLE account ADD COLUMN categories_off_at INTEGER;
