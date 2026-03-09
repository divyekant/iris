-- migrations/007_inbox_stats.sql
-- Pre-computed inbox statistics, refreshed after each sync batch.

CREATE TABLE IF NOT EXISTS inbox_stats (
    account_id TEXT NOT NULL PRIMARY KEY,
    total INTEGER NOT NULL DEFAULT 0,
    unread INTEGER NOT NULL DEFAULT 0,
    starred INTEGER NOT NULL DEFAULT 0,
    by_category TEXT NOT NULL DEFAULT '{}',
    top_senders TEXT NOT NULL DEFAULT '[]',
    today_count INTEGER NOT NULL DEFAULT 0,
    week_count INTEGER NOT NULL DEFAULT 0,
    month_count INTEGER NOT NULL DEFAULT 0,
    last_updated INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT OR IGNORE INTO schema_version (version) VALUES (7);
