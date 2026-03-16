-- Entity extraction and knowledge graph
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    canonical_name TEXT NOT NULL,
    entity_type TEXT NOT NULL CHECK(entity_type IN ('person', 'org', 'project', 'date', 'amount')),
    confidence REAL DEFAULT 0.8,
    created_at INTEGER DEFAULT (unixepoch()),
    updated_at INTEGER DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS entity_aliases (
    id TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL,
    alias_name TEXT NOT NULL,
    source_message_id TEXT,
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(entity_id) REFERENCES entities(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS entity_relations (
    id TEXT PRIMARY KEY,
    entity_a TEXT NOT NULL,
    entity_b TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    weight REAL DEFAULT 1.0,
    source_message_id TEXT,
    created_at INTEGER DEFAULT (unixepoch()),
    FOREIGN KEY(entity_a) REFERENCES entities(id) ON DELETE CASCADE,
    FOREIGN KEY(entity_b) REFERENCES entities(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_entity_aliases_entity ON entity_aliases(entity_id);
CREATE INDEX IF NOT EXISTS idx_entity_aliases_name ON entity_aliases(alias_name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_entity_relations_a ON entity_relations(entity_a);
CREATE INDEX IF NOT EXISTS idx_entity_relations_b ON entity_relations(entity_b);

-- Add entity_extract to the job queue CHECK constraint
-- SQLite doesn't support ALTER CHECK, so we recreate the column constraint via a new table approach
-- Instead, we just allow entity_extract jobs by creating a migration note;
-- the CHECK constraint update is handled in migration 053 which drops and recreates the table
-- For now, entity_extract jobs are handled at the application level

INSERT INTO schema_version (version) VALUES (52);
