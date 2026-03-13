CREATE TABLE IF NOT EXISTS mcp_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL UNIQUE,
    account_id TEXT NOT NULL,
    api_key_id INTEGER, -- optional link to api_keys table
    capabilities TEXT NOT NULL DEFAULT '[]', -- JSON array of enabled tools
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_active_at TEXT NOT NULL DEFAULT (datetime('now')),
    is_active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS mcp_tool_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    input_params TEXT NOT NULL, -- JSON
    output_result TEXT, -- JSON
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'success', 'error'
    duration_ms INTEGER,
    called_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_mcp_sessions_active ON mcp_sessions(is_active);
CREATE INDEX IF NOT EXISTS idx_mcp_tool_calls_session ON mcp_tool_calls(session_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (50);
