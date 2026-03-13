-- migrations/038_followup_tracking.sql

CREATE TABLE IF NOT EXISTS followup_tracking (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    thread_id TEXT,
    to_address TEXT NOT NULL,
    subject TEXT,
    sent_at INTEGER NOT NULL,
    followup_after INTEGER NOT NULL, -- epoch when follow-up is due
    status TEXT NOT NULL DEFAULT 'active', -- 'active', 'replied', 'followed_up', 'cancelled'
    note TEXT,
    reply_message_id TEXT, -- set when reply detected
    reply_detected_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(message_id, to_address)
);

CREATE INDEX IF NOT EXISTS idx_followup_status ON followup_tracking(status);
CREATE INDEX IF NOT EXISTS idx_followup_due ON followup_tracking(followup_after) WHERE status = 'active';

INSERT INTO schema_version (version) VALUES (38);
