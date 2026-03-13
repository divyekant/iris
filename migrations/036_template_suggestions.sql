-- migrations/036_template_suggestions.sql
-- Feature #69: Template auto-generation from repeated patterns

-- Templates table (reusable email templates)
CREATE TABLE IF NOT EXISTS templates (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- AI-detected template suggestions from sent email patterns
CREATE TABLE IF NOT EXISTS template_suggestions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    subject_pattern TEXT,
    body_pattern TEXT NOT NULL,
    sample_message_ids TEXT NOT NULL, -- JSON array of source message IDs
    pattern_count INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.0,
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'accepted', 'dismissed'
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    accepted_at INTEGER,
    dismissed_at INTEGER
);

INSERT INTO schema_version (version) VALUES (36);
