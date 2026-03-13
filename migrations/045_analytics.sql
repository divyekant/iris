-- migrations/045_analytics.sql

CREATE TABLE IF NOT EXISTS analytics_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    snapshot_date TEXT NOT NULL,
    metrics TEXT NOT NULL, -- JSON blob with all computed metrics
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_analytics_snapshots_unique ON analytics_snapshots(account_id, snapshot_date);
CREATE INDEX IF NOT EXISTS idx_analytics_snapshots_account ON analytics_snapshots(account_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (45);
