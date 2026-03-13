-- migrations/037_notification_routing.sql

CREATE TABLE IF NOT EXISTS notification_routing_config (
    id INTEGER PRIMARY KEY CHECK (id = 1), -- singleton row
    push_categories TEXT NOT NULL DEFAULT '[]', -- JSON array of categories that get push
    digest_categories TEXT NOT NULL DEFAULT '[]', -- JSON array for digest
    silent_categories TEXT NOT NULL DEFAULT '[]', -- JSON array for silent
    push_senders TEXT NOT NULL DEFAULT '[]', -- JSON array of VIP senders that always get push
    digest_interval_minutes INTEGER NOT NULL DEFAULT 60,
    quiet_hours_start TEXT, -- HH:MM format, nullable
    quiet_hours_end TEXT,
    vip_always_push INTEGER NOT NULL DEFAULT 1,
    urgency_threshold TEXT NOT NULL DEFAULT 'high', -- 'low', 'normal', 'high', 'urgent'
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS notification_digest_items (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    from_address TEXT,
    subject TEXT,
    category TEXT,
    priority TEXT,
    route TEXT NOT NULL DEFAULT 'digest', -- 'push', 'digest', 'silent'
    is_read INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Insert default config
INSERT OR IGNORE INTO notification_routing_config (id) VALUES (1);

INSERT INTO schema_version (version) VALUES (37);
