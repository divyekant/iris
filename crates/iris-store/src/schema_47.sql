-- Migration 47 (decision D1, issues #121, #579 and #241): the time zone of
-- the owner, as an IANA name (for example `Europe/Berlin`). With no row, the
-- engine uses the time zone of the computer. `iris zone` writes the row.
CREATE TABLE IF NOT EXISTS owner_setting (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  time_zone TEXT NOT NULL
);
