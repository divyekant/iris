CREATE TABLE IF NOT EXISTS blocked_senders (
    id TEXT PRIMARY KEY,
    email_address TEXT NOT NULL UNIQUE,
    reason TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_blocked_email ON blocked_senders(email_address);
INSERT OR IGNORE INTO schema_version (version) VALUES (13);
