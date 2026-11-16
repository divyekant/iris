-- Migration 12: the days of use for the measures page (card R5-12, the
-- manifesto "How we measure Iris").
--
-- One row for each day on which the person used the web client, by the
-- local date of the person. `opens` counts the source mail that the person
-- opened on that day. The rows stay on this machine.

CREATE TABLE use_day (
  day TEXT PRIMARY KEY
    CHECK (day GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(day) IS day),
  opens INTEGER NOT NULL DEFAULT 0 CHECK (opens >= 0)
) WITHOUT ROWID;
