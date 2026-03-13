-- migrations/043_newsletter_feeds.sql
-- Newsletter feed view: group newsletters by sender for magazine-style browsing

-- list_unsubscribe columns already exist from migration 020

CREATE TABLE IF NOT EXISTS newsletter_feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    sender_address TEXT NOT NULL,
    sender_name TEXT,
    display_name TEXT, -- user-customizable name
    is_muted INTEGER NOT NULL DEFAULT 0,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    last_received_at TEXT,
    article_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_newsletter_feeds_unique ON newsletter_feeds(account_id, sender_address);
CREATE INDEX IF NOT EXISTS idx_newsletter_feeds_account ON newsletter_feeds(account_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (43);
