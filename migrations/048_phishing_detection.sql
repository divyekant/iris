-- migrations/048_phishing_detection.sql

CREATE TABLE IF NOT EXISTS phishing_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    account_id INTEGER NOT NULL,
    risk_level TEXT NOT NULL, -- 'safe', 'low', 'medium', 'high', 'critical'
    risk_score REAL NOT NULL DEFAULT 0.0, -- 0.0 to 1.0
    signals TEXT NOT NULL DEFAULT '[]', -- JSON array of detected signals
    ai_analysis TEXT, -- AI-generated explanation
    checked_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_phishing_message ON phishing_reports(message_id);
CREATE INDEX IF NOT EXISTS idx_phishing_account ON phishing_reports(account_id);
CREATE INDEX IF NOT EXISTS idx_phishing_risk ON phishing_reports(risk_level);

INSERT OR IGNORE INTO schema_version (version) VALUES (48);
