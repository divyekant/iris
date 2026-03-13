-- Migration 025: Deadline extraction
CREATE TABLE deadlines (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    thread_id TEXT,
    description TEXT NOT NULL,
    deadline_date TEXT NOT NULL,
    deadline_source TEXT NOT NULL,
    is_explicit INTEGER NOT NULL DEFAULT 1,
    completed INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(message_id, description)
);
CREATE INDEX idx_deadlines_thread ON deadlines(thread_id);
CREATE INDEX idx_deadlines_date ON deadlines(deadline_date);
CREATE INDEX idx_deadlines_completed ON deadlines(completed);

INSERT INTO schema_version (version) VALUES (25);
