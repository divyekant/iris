-- Migration 66 (issue 216): the ledger entries with messages that the
-- recovery cannot confirm (`ledger_message.listed = 'unknown'`), that the
-- person dismissed from the line of the brief (specification 2, section
-- 7.3). A ledger row refuses each update, so a dismissal is a row of its
-- own.
CREATE TABLE IF NOT EXISTS ledger_unknown_seen (
  ledger_id INTEGER PRIMARY KEY REFERENCES ledger(id),
  -- The time of the dismissal, in milliseconds.
  at INTEGER NOT NULL
);
