-- migrations/014_muted_threads.sql

CREATE TABLE IF NOT EXISTS muted_threads (
    thread_id TEXT PRIMARY KEY,
    muted_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

INSERT OR IGNORE INTO schema_version (version) VALUES (14);
