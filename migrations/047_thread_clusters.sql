CREATE TABLE IF NOT EXISTS thread_clusters (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    cluster_name TEXT NOT NULL,
    cluster_type TEXT NOT NULL DEFAULT 'related', -- 'duplicate', 'related', 'followup'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS thread_cluster_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cluster_id INTEGER NOT NULL REFERENCES thread_clusters(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL,
    similarity_score REAL NOT NULL DEFAULT 1.0,
    added_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_thread_clusters_account ON thread_clusters(account_id);
CREATE INDEX IF NOT EXISTS idx_cluster_members_cluster ON thread_cluster_members(cluster_id);
CREATE INDEX IF NOT EXISTS idx_cluster_members_thread ON thread_cluster_members(thread_id);

INSERT OR IGNORE INTO schema_version (version) VALUES (47);
