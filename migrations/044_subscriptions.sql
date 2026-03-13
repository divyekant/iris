-- Migration 044: Subscription management dashboard
-- list_unsubscribe columns already exist from migration 020

-- Centralized table for tracking email subscriptions

CREATE TABLE IF NOT EXISTS subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL,
    sender_address TEXT NOT NULL,
    sender_name TEXT,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    total_count INTEGER NOT NULL DEFAULT 0,
    read_count INTEGER NOT NULL DEFAULT 0,
    unsubscribe_url TEXT, -- from List-Unsubscribe header
    status TEXT NOT NULL DEFAULT 'active', -- 'active', 'unsubscribed', 'archived', 'blocked'
    frequency_days REAL, -- average days between emails
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_subscriptions_unique ON subscriptions(account_id, sender_address);
CREATE INDEX IF NOT EXISTS idx_subscriptions_account ON subscriptions(account_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions(status);

INSERT OR IGNORE INTO schema_version (version) VALUES (44);
