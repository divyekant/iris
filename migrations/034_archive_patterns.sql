-- migrations/034_archive_patterns.sql
-- Feature #64: Auto-archive patterns — learns what emails the user always archives

CREATE TABLE IF NOT EXISTS archive_patterns (
    id TEXT PRIMARY KEY,
    pattern_type TEXT NOT NULL,       -- 'sender', 'category', 'subject_pattern', 'sender_category'
    pattern_value TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    match_count INTEGER NOT NULL DEFAULT 0,
    total_from_sender INTEGER NOT NULL DEFAULT 0,
    archive_rate REAL NOT NULL DEFAULT 0.0,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(pattern_type, pattern_value)
);

INSERT INTO schema_version (version) VALUES (34);
