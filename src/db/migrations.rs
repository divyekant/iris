use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_chat.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_agent.sql");
const MIGRATION_004: &str = include_str!("../../migrations/004_ai_feedback.sql");
const MIGRATION_005: &str = include_str!("../../migrations/005_job_queue.sql");
const MIGRATION_006: &str = include_str!("../../migrations/006_dedup_messages.sql");
const MIGRATION_007: &str = include_str!("../../migrations/007_inbox_stats.sql");
const MIGRATION_008: &str = include_str!("../../migrations/008_pending_sends.sql");
const MIGRATION_009: &str = include_str!("../../migrations/009_signatures.sql");
const MIGRATION_010: &str = include_str!("../../migrations/010_snooze.sql");
const MIGRATION_011: &str = include_str!("../../migrations/011_attachments.sql");
const MIGRATION_012: &str = include_str!("../../migrations/012_templates.sql");

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

    if current_version < 5 {
        conn.execute_batch(MIGRATION_005)?;
        tracing::info!("Applied migration 005_job_queue");
    }

    if current_version < 6 {
        conn.execute_batch(MIGRATION_006)?;
        tracing::info!("Applied migration 006_dedup_messages");
    }

    if current_version < 7 {
        conn.execute_batch(MIGRATION_007)?;
        tracing::info!("Applied migration 007_inbox_stats");
    }

    if current_version < 8 {
        conn.execute_batch(MIGRATION_008)?;
        tracing::info!("Applied migration 008_pending_sends");
    }

    if current_version < 9 {
        conn.execute_batch(MIGRATION_009)?;
        tracing::info!("Applied migration 009_signatures");
    }

    if current_version < 10 {
        conn.execute_batch(MIGRATION_010)?;
        tracing::info!("Applied migration 010_snooze");
    }

    if current_version < 11 {
        conn.execute_batch(MIGRATION_011)?;
        tracing::info!("Applied migration 011_attachments");
    }

    if current_version < 12 {
        conn.execute_batch(MIGRATION_012)?;
        tracing::info!("Applied migration 012_templates");
    }

    Ok(())
}
