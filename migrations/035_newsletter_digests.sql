-- migrations/035_newsletter_digests.sql

CREATE TABLE IF NOT EXISTS newsletter_digests (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    message_ids TEXT NOT NULL, -- JSON array of included message IDs
    source_count INTEGER NOT NULL DEFAULT 0,
    message_count INTEGER NOT NULL DEFAULT 0,
    date_from INTEGER,
    date_to INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO schema_version (version) VALUES (35);
