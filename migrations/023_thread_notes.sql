CREATE TABLE IF NOT EXISTS thread_notes (
    id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(8)))),
    thread_id TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_thread_notes_thread ON thread_notes(thread_id);
INSERT OR IGNORE INTO schema_version (version) VALUES (23);
