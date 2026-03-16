-- Timeline events for temporal reasoning
CREATE TABLE IF NOT EXISTS timeline_events (
    id TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    approximate_date TEXT NOT NULL,
    date_precision TEXT DEFAULT 'day' CHECK(date_precision IN ('day', 'week', 'month', 'quarter', 'year')),
    source_message_id TEXT,
    account_id TEXT,
    confidence REAL DEFAULT 0.7,
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_timeline_events_date ON timeline_events(approximate_date);
CREATE INDEX IF NOT EXISTS idx_timeline_events_name ON timeline_events(event_name COLLATE NOCASE);

-- Recreate processing_jobs with updated CHECK constraint to include entity_extract
CREATE TABLE IF NOT EXISTS processing_jobs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract','entity_extract')),
    message_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','processing','done','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 4,
    payload TEXT,
    error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_retry_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT INTO processing_jobs_new (id, job_type, message_id, status, attempts, max_attempts, payload, error, created_at, updated_at, next_retry_at)
    SELECT id, job_type, message_id, status, attempts, max_attempts, payload, error, created_at, updated_at, next_retry_at
    FROM processing_jobs;

DROP TABLE processing_jobs;
ALTER TABLE processing_jobs_new RENAME TO processing_jobs;

CREATE INDEX IF NOT EXISTS idx_jobs_poll ON processing_jobs(status, next_retry_at) WHERE status IN ('pending','processing');
CREATE INDEX IF NOT EXISTS idx_jobs_message ON processing_jobs(message_id, job_type);

INSERT INTO schema_version (version) VALUES (53);
