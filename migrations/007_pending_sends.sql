-- Migration 007: Pending sends (undo-send and scheduled send)
CREATE TABLE IF NOT EXISTS pending_sends (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_addresses TEXT NOT NULL,        -- JSON array
    cc_addresses TEXT,                  -- JSON array
    bcc_addresses TEXT,                 -- JSON array
    subject TEXT NOT NULL DEFAULT '',
    body_text TEXT NOT NULL DEFAULT '',
    body_html TEXT,
    in_reply_to TEXT,
    references_header TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    send_at INTEGER NOT NULL,           -- epoch seconds when to actually send
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','sending','sent','cancelled','failed')),
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_pending_sends_status_send_at ON pending_sends(status, send_at);

INSERT INTO schema_version (version) VALUES (7);
