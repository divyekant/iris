-- Job queue for reliable async processing with retry
CREATE TABLE IF NOT EXISTS processing_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract')),
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

CREATE INDEX idx_jobs_poll ON processing_jobs(status, next_retry_at) WHERE status IN ('pending','processing');
CREATE INDEX idx_jobs_message ON processing_jobs(message_id, job_type);

-- Track processing status on messages
ALTER TABLE messages ADD COLUMN ai_status TEXT DEFAULT NULL;
ALTER TABLE messages ADD COLUMN memories_status TEXT DEFAULT NULL;

INSERT INTO schema_version (version) VALUES (5);
