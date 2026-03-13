-- migrations/049_contact_profiles.sql

CREATE TABLE IF NOT EXISTS contact_profiles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    email_address TEXT NOT NULL,
    display_name TEXT,
    organization TEXT,
    first_seen_at TEXT,
    last_seen_at TEXT,
    total_emails_from INTEGER NOT NULL DEFAULT 0,
    total_emails_to INTEGER NOT NULL DEFAULT 0,
    avg_response_time_hours REAL,
    top_categories TEXT, -- JSON array
    communication_style TEXT, -- 'formal', 'casual', 'mixed'
    ai_summary TEXT, -- AI-generated profile summary
    profile_data TEXT NOT NULL DEFAULT '{}', -- JSON blob for extensible data
    generated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_contact_profiles_unique ON contact_profiles(account_id, email_address);
CREATE INDEX IF NOT EXISTS idx_contact_profiles_account ON contact_profiles(account_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (49);
