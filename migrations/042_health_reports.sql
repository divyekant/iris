-- migrations/042_health_reports.sql

CREATE TABLE IF NOT EXISTS health_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    report_type TEXT NOT NULL, -- 'weekly', 'monthly', 'custom'
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    report_data TEXT NOT NULL, -- JSON blob with all metrics
    insights TEXT, -- AI-generated insights
    generated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_health_reports_account ON health_reports(account_id);
CREATE INDEX IF NOT EXISTS idx_health_reports_period ON health_reports(period_start, period_end);

INSERT OR IGNORE INTO schema_version (version) VALUES (42);
