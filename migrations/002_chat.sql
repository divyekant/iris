-- migrations/002_chat.sql

-- Chat conversation messages for AI chat panel
CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user', 'assistant')),
    content TEXT NOT NULL,
    citations TEXT,          -- JSON array of message IDs referenced
    proposed_action TEXT,    -- JSON: { "action": "archive", "query": "...", "count": N }
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_chat_session ON chat_messages(session_id, created_at);

INSERT INTO schema_version (version) VALUES (2);
