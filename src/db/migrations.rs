use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_chat.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_agent.sql");
const MIGRATION_004: &str = include_str!("../../migrations/004_ai_feedback.sql");

pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Ensure schema_version table exists before querying (handles fresh databases)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL DEFAULT (unixepoch())
        );",
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        conn.execute_batch(MIGRATION_001)?;
        tracing::info!("Applied migration 001_initial");
    }

    if current_version < 2 {
        conn.execute_batch(MIGRATION_002)?;
        tracing::info!("Applied migration 002_chat");
    }

    if current_version < 3 {
        conn.execute_batch(MIGRATION_003)?;
        tracing::info!("Applied migration 003_agent");
    }

    if current_version < 4 {
        conn.execute_batch(MIGRATION_004)?;
        tracing::info!("Applied migration 004_ai_feedback");
    }

    Ok(())
}
