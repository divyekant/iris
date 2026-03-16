CREATE TABLE IF NOT EXISTS writing_style (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    trait_type TEXT NOT NULL CHECK(trait_type IN ('greeting', 'signoff', 'tone', 'avg_length', 'formality', 'vocabulary')),
    trait_value TEXT NOT NULL,
    confidence REAL DEFAULT 0.5,
    examples TEXT, -- JSON array of example snippets
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id)
);

CREATE INDEX IF NOT EXISTS idx_writing_style_account ON writing_style(account_id);

-- Recreate processing_jobs with updated CHECK constraint to include style_extract and auto_draft
CREATE TABLE IF NOT EXISTS processing_jobs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract','entity_extract','style_extract','auto_draft')),
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

INSERT INTO schema_version (version) VALUES (54);
