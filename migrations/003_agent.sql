-- migrations/003_agent.sql

-- API keys for agent access (S13)
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_prefix TEXT NOT NULL,
    permission TEXT NOT NULL DEFAULT 'read_only'
        CHECK(permission IN ('read_only', 'draft_only', 'send_with_approval', 'autonomous')),
    account_id TEXT REFERENCES accounts(id),
    is_revoked INTEGER DEFAULT 0,
    last_used_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    revoked_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

-- Audit log for agent actions (S17)
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_id TEXT NOT NULL REFERENCES api_keys(id),
    action TEXT NOT NULL,
    resource_type TEXT,
    resource_id TEXT,
    details TEXT,
    status TEXT NOT NULL DEFAULT 'success',
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_audit_log_key ON audit_log(api_key_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_log_time ON audit_log(created_at DESC);

INSERT INTO schema_version (version) VALUES (3);
