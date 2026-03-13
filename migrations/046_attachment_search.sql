-- migrations/046_attachment_search.sql
-- Attachment content search: storage table, text cache, FTS5 index

-- attachments table already exists from migration 011
-- (id TEXT PK, message_id, filename, content_type, size, content_id, data BLOB)
-- Add account_id column for efficient filtering
ALTER TABLE attachments ADD COLUMN account_id TEXT;

-- FTS5 contentless index for attachment text search
CREATE VIRTUAL TABLE IF NOT EXISTS attachment_content_fts USING fts5(
    attachment_id,
    message_id,
    filename,
    content_text
);

-- Cache table for extracted text from attachments
CREATE TABLE IF NOT EXISTS attachment_text_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attachment_id TEXT NOT NULL UNIQUE REFERENCES attachments(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    filename TEXT,
    content_text TEXT NOT NULL,
    extracted_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_attachment_text_message ON attachment_text_cache(message_id);
CREATE INDEX IF NOT EXISTS idx_attachment_text_account ON attachment_text_cache(account_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (46);
