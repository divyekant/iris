-- migrations/001_initial.sql

-- Account configurations (S11)
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,          -- 'gmail', 'outlook', 'yahoo', 'fastmail', 'imap'
    email TEXT NOT NULL UNIQUE,
    display_name TEXT,
    -- OAuth2 tokens (null for password auth)
    access_token TEXT,
    refresh_token TEXT,
    token_expires_at INTEGER,        -- unix timestamp
    -- IMAP/SMTP config (for manual setup)
    imap_host TEXT,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT,
    smtp_port INTEGER DEFAULT 587,
    username TEXT,
    password_encrypted TEXT,
    -- Sync state
    last_sync_at INTEGER,
    sync_status TEXT DEFAULT 'pending', -- pending, syncing, idle, error
    sync_error TEXT,
    -- Metadata
    is_active INTEGER DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Core email store (S5) — nullable AI columns for V6+
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    message_id TEXT,                 -- RFC Message-ID header
    thread_id TEXT,                  -- grouped by References/In-Reply-To
    folder TEXT NOT NULL DEFAULT 'INBOX',
    -- Headers
    from_address TEXT,
    from_name TEXT,
    to_addresses TEXT,               -- JSON array
    cc_addresses TEXT,               -- JSON array
    bcc_addresses TEXT,              -- JSON array
    subject TEXT,
    date INTEGER,                    -- unix timestamp from Date header
    -- Body
    snippet TEXT,                    -- first ~200 chars plaintext
    body_text TEXT,                  -- plaintext body
    body_html TEXT,                  -- HTML body
    -- Flags
    is_read INTEGER DEFAULT 0,
    is_starred INTEGER DEFAULT 0,
    is_draft INTEGER DEFAULT 0,
    is_deleted INTEGER DEFAULT 0,
    labels TEXT,                     -- JSON array of label strings
    -- IMAP state
    uid INTEGER,                     -- IMAP UID
    modseq INTEGER,                  -- IMAP MODSEQ for sync
    raw_headers TEXT,                -- full headers for auth parsing (V9)
    -- AI metadata (nullable — populated in V6)
    ai_intent TEXT,                  -- ACTION_REQUEST, INFORMATIONAL, etc.
    ai_priority_score REAL,          -- 0.0-1.0 Eisenhower score
    ai_priority_label TEXT,          -- urgent, high, normal, low
    ai_category TEXT,                -- dynamic AI category
    ai_entities TEXT,                -- JSON: extracted people, dates, amounts
    ai_deadline TEXT,                -- extracted deadline ISO string
    ai_summary TEXT,                 -- thread summary (V7, cached)
    -- Metadata
    has_attachments INTEGER DEFAULT 0,
    attachment_names TEXT,           -- JSON array of filenames
    size_bytes INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_messages_account ON messages(account_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(account_id, folder);
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date DESC);
CREATE INDEX IF NOT EXISTS idx_messages_uid ON messages(account_id, folder, uid);

-- FTS5 full-text index (S10) — populated during sync, searched in V5
CREATE VIRTUAL TABLE IF NOT EXISTS fts_messages USING fts5(
    message_id,
    subject,
    body_text,
    from_address,
    from_name,
    content=messages,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Triggers to keep FTS5 in sync
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO fts_messages(rowid, message_id, subject, body_text, from_address, from_name)
    VALUES (new.rowid, new.message_id, new.subject, new.body_text, new.from_address, new.from_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO fts_messages(fts_messages, rowid, message_id, subject, body_text, from_address, from_name)
    VALUES ('delete', old.rowid, old.message_id, old.subject, old.body_text, old.from_address, old.from_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO fts_messages(fts_messages, rowid, message_id, subject, body_text, from_address, from_name)
    VALUES ('delete', old.rowid, old.message_id, old.subject, old.body_text, old.from_address, old.from_name);
    INSERT INTO fts_messages(rowid, message_id, subject, body_text, from_address, from_name)
    VALUES (new.rowid, new.message_id, new.subject, new.body_text, new.from_address, new.from_name);
END;

-- App config (S12) — key-value store for theme (S4) and future settings
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Default theme
INSERT OR IGNORE INTO config (key, value) VALUES ('theme', 'system');

-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
