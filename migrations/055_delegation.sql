CREATE TABLE IF NOT EXISTS delegation_playbooks (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    trigger_conditions TEXT NOT NULL, -- JSON: { sender_domain?, subject_contains?, category?, intent? }
    action_type TEXT NOT NULL CHECK(action_type IN ('auto_reply', 'draft_reply', 'forward', 'archive', 'label')),
    action_template TEXT, -- template body for reply/draft actions
    confidence_threshold REAL DEFAULT 0.85,
    enabled INTEGER DEFAULT 1,
    match_count INTEGER DEFAULT 0,
    last_matched_at INTEGER,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(account_id) REFERENCES accounts(id)
);

CREATE TABLE IF NOT EXISTS delegation_actions (
    id TEXT PRIMARY KEY,
    playbook_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    action_taken TEXT NOT NULL,
    confidence REAL NOT NULL,
    status TEXT DEFAULT 'completed' CHECK(status IN ('completed', 'undone', 'pending_review')),
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(playbook_id) REFERENCES delegation_playbooks(id),
    FOREIGN KEY(message_id) REFERENCES messages(id)
);

CREATE INDEX IF NOT EXISTS idx_delegation_playbooks_account ON delegation_playbooks(account_id);
CREATE INDEX IF NOT EXISTS idx_delegation_actions_message ON delegation_actions(message_id);
CREATE INDEX IF NOT EXISTS idx_delegation_actions_playbook ON delegation_actions(playbook_id);

-- Recreate processing_jobs with updated CHECK constraint to include delegation_process
CREATE TABLE IF NOT EXISTS processing_jobs_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract','entity_extract','style_extract','auto_draft','delegation_process')),
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

INSERT INTO schema_version (version) VALUES (55);
