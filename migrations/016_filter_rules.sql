-- Filter rules: auto-apply actions to incoming messages based on conditions
CREATE TABLE IF NOT EXISTS filter_rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    conditions TEXT NOT NULL DEFAULT '[]',  -- JSON array of {field, operator, value}
    actions TEXT NOT NULL DEFAULT '[]',     -- JSON array of {type, value?}
    is_active INTEGER NOT NULL DEFAULT 1,
    account_id TEXT,  -- NULL = all accounts
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

INSERT OR IGNORE INTO schema_version (version) VALUES (16);
