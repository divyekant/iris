-- migrations/008_pending_sends.sql

CREATE TABLE IF NOT EXISTS pending_sends (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    to_addresses TEXT NOT NULL,     -- JSON array
    cc_addresses TEXT,              -- JSON array
    bcc_addresses TEXT,             -- JSON array
    subject TEXT,
    body_text TEXT,
    body_html TEXT,
    in_reply_to TEXT,
    references_header TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    send_at INTEGER NOT NULL,       -- when to actually send (created_at + delay)
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','cancelled','sent','failed'))
);

CREATE INDEX IF NOT EXISTS idx_pending_sends_status ON pending_sends(status, send_at);

-- Default undo-send delay of 10 seconds
INSERT OR IGNORE INTO config (key, value) VALUES ('undo_send_delay_seconds', '10');

INSERT OR IGNORE INTO schema_version (version) VALUES (8);
