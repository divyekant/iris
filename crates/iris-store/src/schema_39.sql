-- Issue 166: one address form for `header.from_addr`, ASCII lowercase, the
-- form of the header parser and of `lower(sender.address)` in the sender
-- triggers of migration 8. These triggers compare with `=`, so the index
-- of `from_addr` serves. A writer that stores another case (a direct SQL
-- write, or a future write path) gets the normal form from these triggers.
-- SQLite `lower()` changes only ASCII letters, as the parser does.

UPDATE header SET from_addr = lower(from_addr) WHERE from_addr <> lower(from_addr);

CREATE TRIGGER header_from_addr_lower_add AFTER INSERT ON header
  WHEN NEW.from_addr <> lower(NEW.from_addr)
BEGIN
  UPDATE header SET from_addr = lower(NEW.from_addr) WHERE message_id = NEW.message_id;
END;

CREATE TRIGGER header_from_addr_lower AFTER UPDATE OF from_addr ON header
  WHEN NEW.from_addr <> lower(NEW.from_addr)
BEGIN
  UPDATE header SET from_addr = lower(NEW.from_addr) WHERE message_id = NEW.message_id;
END;
