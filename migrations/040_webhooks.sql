-- migrations/040_webhooks.sql

CREATE TABLE IF NOT EXISTS webhooks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    secret TEXT, -- optional HMAC secret for signature verification
    events TEXT NOT NULL, -- comma-separated event types
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_triggered_at TEXT,
    failure_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    webhook_id INTEGER NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL, -- JSON payload sent
    status_code INTEGER, -- HTTP response code
    response_body TEXT, -- truncated response
    success INTEGER NOT NULL DEFAULT 0,
    delivered_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_webhooks_account ON webhooks(account_id);
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_webhook ON webhook_deliveries(webhook_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (40);
