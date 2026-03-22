CREATE TABLE IF NOT EXISTS custom_categories (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    is_ai_generated INTEGER DEFAULT 0,
    email_count INTEGER DEFAULT 0,
    status TEXT DEFAULT 'active' CHECK(status IN ('active', 'suggested', 'dismissed')),
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_custom_categories_account ON custom_categories(account_id);
CREATE INDEX IF NOT EXISTS idx_custom_categories_status ON custom_categories(status);

INSERT INTO schema_version (version) VALUES (57);
