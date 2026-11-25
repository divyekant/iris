-- Issue 812: the name triggers of migration 54 (`fixed_due_name`, `fixed_due_name_add`, `fixed_due_name_remove`) find
-- the mail under a known person name with `h.from_name = name COLLATE
-- NOCASE`. This index lets each such trigger read only the rows of that
-- name, not the whole `header` table.
CREATE INDEX IF NOT EXISTS header_from_name_nocase ON header(from_name COLLATE NOCASE);
