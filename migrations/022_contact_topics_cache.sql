-- migrations/022_contact_topics_cache.sql
-- Cache table for AI-generated contact topic summaries (1-hour TTL)
CREATE TABLE IF NOT EXISTS contact_topics_cache (
    email TEXT PRIMARY KEY,
    topics_json TEXT NOT NULL,      -- JSON array of {topic, count}
    total_emails INTEGER NOT NULL,
    computed_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO schema_version (version) VALUES (22);
