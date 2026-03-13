-- migrations/028_relationship_priority.sql

CREATE TABLE IF NOT EXISTS relationship_scores (
    email TEXT PRIMARY KEY,
    score REAL NOT NULL DEFAULT 0.0,
    frequency_score REAL NOT NULL DEFAULT 0.0,
    recency_score REAL NOT NULL DEFAULT 0.0,
    reply_rate_score REAL NOT NULL DEFAULT 0.0,
    bidirectional_score REAL NOT NULL DEFAULT 0.0,
    thread_depth_score REAL NOT NULL DEFAULT 0.0,
    computed_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO schema_version (version) VALUES (28);
