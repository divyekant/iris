CREATE TABLE IF NOT EXISTS social_engineering_analysis (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL,
    risk_level TEXT NOT NULL DEFAULT 'none',
    tactics_json TEXT,
    summary TEXT,
    analyzed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(message_id)
);

INSERT INTO schema_version (version) VALUES (29);
