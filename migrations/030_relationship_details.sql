-- migrations/030_relationship_details.sql
-- Richer relationship scoring with per-factor breakdowns and strength labels

CREATE TABLE IF NOT EXISTS relationship_details (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL,
    email TEXT NOT NULL,
    display_name TEXT,
    strength_label TEXT NOT NULL CHECK (strength_label IN ('strong', 'regular', 'weak', 'dormant')),
    overall_score REAL NOT NULL,
    frequency_score REAL DEFAULT 0,
    recency_score REAL DEFAULT 0,
    reciprocity_score REAL DEFAULT 0,
    response_time_score REAL DEFAULT 0,
    thread_engagement_score REAL DEFAULT 0,
    total_sent INTEGER DEFAULT 0,
    total_received INTEGER DEFAULT 0,
    avg_response_time_secs INTEGER,
    last_sent INTEGER,
    last_received INTEGER,
    first_interaction INTEGER,
    computed_at INTEGER NOT NULL,
    UNIQUE(account_id, email)
);

CREATE INDEX IF NOT EXISTS idx_relationship_details_account ON relationship_details(account_id);
CREATE INDEX IF NOT EXISTS idx_relationship_details_score ON relationship_details(overall_score DESC);
CREATE INDEX IF NOT EXISTS idx_relationship_details_strength ON relationship_details(strength_label);

INSERT INTO schema_version (version) VALUES (30);
