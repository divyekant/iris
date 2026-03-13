-- migrations/026_vip_contacts.sql

CREATE TABLE IF NOT EXISTS vip_contacts (
    email TEXT PRIMARY KEY,
    display_name TEXT,
    vip_score REAL NOT NULL DEFAULT 0.0,
    is_manual INTEGER NOT NULL DEFAULT 0,
    message_count INTEGER NOT NULL DEFAULT 0,
    reply_count INTEGER NOT NULL DEFAULT 0,
    last_contact INTEGER,
    first_contact INTEGER,
    avg_reply_time_secs INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_vip_score ON vip_contacts(vip_score DESC);
CREATE INDEX IF NOT EXISTS idx_vip_manual ON vip_contacts(is_manual);

INSERT OR IGNORE INTO schema_version (version) VALUES (26);
