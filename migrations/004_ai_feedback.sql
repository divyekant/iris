-- migrations/004_ai_feedback.sql

-- User corrections for AI classifications (feedback loop)
CREATE TABLE IF NOT EXISTS ai_feedback (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL REFERENCES messages(id),
    field TEXT NOT NULL,              -- 'category', 'priority_label', 'intent'
    original_value TEXT,
    corrected_value TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_ai_feedback_field ON ai_feedback(field, corrected_value);

INSERT INTO schema_version (version) VALUES (4);
