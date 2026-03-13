-- migrations/024_draft_versions.sql

CREATE TABLE IF NOT EXISTS draft_versions (
    id INTEGER PRIMARY KEY,
    draft_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    version_number INTEGER NOT NULL,
    subject TEXT,
    body TEXT,
    to_addresses TEXT,    -- JSON array
    cc_addresses TEXT,    -- JSON array
    word_count INTEGER DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(draft_id, version_number)
);

CREATE INDEX IF NOT EXISTS idx_draft_versions_draft ON draft_versions(draft_id, version_number DESC);

INSERT OR IGNORE INTO schema_version (version) VALUES (31);
