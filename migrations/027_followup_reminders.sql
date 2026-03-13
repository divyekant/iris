-- Migration 027: Follow-up Reminders
-- AI-identified emails that warrant follow-up

CREATE TABLE followup_reminders (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(message_id),
    thread_id TEXT,
    reason TEXT NOT NULL,
    suggested_date TEXT NOT NULL,
    urgency TEXT NOT NULL DEFAULT 'normal',
    status TEXT NOT NULL DEFAULT 'pending',
    snoozed_until TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    dismissed_at INTEGER,
    acted_at INTEGER
);
CREATE INDEX idx_followup_status ON followup_reminders(status);
CREATE INDEX idx_followup_date ON followup_reminders(suggested_date);
CREATE INDEX idx_followup_message ON followup_reminders(message_id);

INSERT INTO schema_version (version) VALUES (27);
