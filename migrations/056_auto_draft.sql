CREATE TABLE IF NOT EXISTS auto_draft_patterns (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    pattern_hash TEXT NOT NULL,
    trigger_description TEXT NOT NULL,
    template_body TEXT NOT NULL,
    match_count INTEGER DEFAULT 0,
    success_rate REAL DEFAULT 0.5,
    last_matched_at INTEGER,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id)
);

CREATE TABLE IF NOT EXISTS auto_drafts (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    pattern_id TEXT,
    draft_body TEXT NOT NULL,
    status TEXT DEFAULT 'pending' CHECK(status IN ('pending', 'used', 'dismissed', 'edited')),
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(message_id) REFERENCES messages(id),
    FOREIGN KEY(account_id) REFERENCES accounts(id),
    FOREIGN KEY(pattern_id) REFERENCES auto_draft_patterns(id)
);

CREATE INDEX IF NOT EXISTS idx_auto_drafts_message ON auto_drafts(message_id);
CREATE INDEX IF NOT EXISTS idx_auto_draft_patterns_account ON auto_draft_patterns(account_id);

INSERT INTO schema_version (version) VALUES (56);
