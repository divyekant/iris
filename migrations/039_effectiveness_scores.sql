CREATE TABLE IF NOT EXISTS effectiveness_scores (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    draft_id TEXT,
    subject TEXT,
    overall_score REAL NOT NULL,
    clarity_score REAL NOT NULL,
    tone_score REAL NOT NULL,
    length_score REAL NOT NULL,
    subject_score REAL NOT NULL,
    cta_score REAL NOT NULL,
    feedback TEXT NOT NULL, -- JSON with detailed feedback
    tips TEXT, -- JSON array of improvement tips
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
INSERT INTO schema_version (version) VALUES (39);
