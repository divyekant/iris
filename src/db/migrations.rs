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
const MIGRATION_013: &str = include_str!("../../migrations/013_blocked_senders.sql");
const MIGRATION_014: &str = include_str!("../../migrations/014_muted_threads.sql");
const MIGRATION_015: &str = include_str!("../../migrations/015_saved_searches.sql");
const MIGRATION_016: &str = include_str!("../../migrations/016_filter_rules.sql");
const MIGRATION_017: &str = include_str!("../../migrations/017_aliases.sql");
const MIGRATION_018: &str = include_str!("../../migrations/018_labels.sql");
const MIGRATION_019: &str = include_str!("../../migrations/019_sentiment.sql");
const MIGRATION_020: &str = include_str!("../../migrations/020_unsubscribe.sql");
const MIGRATION_021: &str = include_str!("../../migrations/021_needs_reply.sql");
const MIGRATION_022: &str = include_str!("../../migrations/022_contact_topics_cache.sql");
const MIGRATION_023: &str = include_str!("../../migrations/023_thread_notes.sql");
const MIGRATION_024: &str = include_str!("../../migrations/024_intent_detection.sql");
const MIGRATION_025: &str = include_str!("../../migrations/025_deadlines.sql");
const MIGRATION_026: &str = include_str!("../../migrations/026_vip_contacts.sql");
const MIGRATION_027: &str = include_str!("../../migrations/027_followup_reminders.sql");
const MIGRATION_028: &str = include_str!("../../migrations/028_relationship_priority.sql");
const MIGRATION_029: &str = include_str!("../../migrations/029_social_engineering.sql");
const MIGRATION_030: &str = include_str!("../../migrations/030_relationship_details.sql");
const MIGRATION_031: &str = include_str!("../../migrations/031_draft_versions.sql");
const MIGRATION_032: &str = include_str!("../../migrations/032_relationship_scores.sql");
const MIGRATION_033: &str = include_str!("../../migrations/033_tracking_pixels.sql");
const MIGRATION_034: &str = include_str!("../../migrations/034_archive_patterns.sql");
const MIGRATION_035: &str = include_str!("../../migrations/035_newsletter_digests.sql");
const MIGRATION_036: &str = include_str!("../../migrations/036_template_suggestions.sql");
const MIGRATION_037: &str = include_str!("../../migrations/037_notification_routing.sql");
const MIGRATION_038: &str = include_str!("../../migrations/038_followup_tracking.sql");
const MIGRATION_039: &str = include_str!("../../migrations/039_effectiveness_scores.sql");
const MIGRATION_040: &str = include_str!("../../migrations/040_webhooks.sql");
const MIGRATION_041: &str = include_str!("../../migrations/041_extracted_data.sql");
const MIGRATION_042: &str = include_str!("../../migrations/042_health_reports.sql");
const MIGRATION_043: &str = include_str!("../../migrations/043_newsletter_feeds.sql");
const MIGRATION_044: &str = include_str!("../../migrations/044_subscriptions.sql");
const MIGRATION_045: &str = include_str!("../../migrations/045_analytics.sql");
const MIGRATION_046: &str = include_str!("../../migrations/046_attachment_search.sql");
const MIGRATION_047: &str = include_str!("../../migrations/047_thread_clusters.sql");
const MIGRATION_048: &str = include_str!("../../migrations/048_phishing_detection.sql");
const MIGRATION_049: &str = include_str!("../../migrations/049_contact_profiles.sql");
const MIGRATION_050: &str = include_str!("../../migrations/050_mcp_server.sql");

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

    if current_version < 13 {
        conn.execute_batch(MIGRATION_013)?;
        tracing::info!("Applied migration 013_blocked_senders");
    }

    if current_version < 14 {
        conn.execute_batch(MIGRATION_014)?;
        tracing::info!("Applied migration 014_muted_threads");
    }

    if current_version < 15 {
        conn.execute_batch(MIGRATION_015)?;
        tracing::info!("Applied migration 015_saved_searches");
    }

    if current_version < 16 {
        conn.execute_batch(MIGRATION_016)?;
        tracing::info!("Applied migration 016_filter_rules");
    }

    if current_version < 17 {
        conn.execute_batch(MIGRATION_017)?;
        tracing::info!("Applied migration 017_aliases");
    }

    if current_version < 18 {
        conn.execute_batch(MIGRATION_018)?;
        tracing::info!("Applied migration 018_labels");
    }

    if current_version < 19 {
        conn.execute_batch(MIGRATION_019)?;
        tracing::info!("Applied migration 019_sentiment");
    }

    if current_version < 20 {
        conn.execute_batch(MIGRATION_020)?;
        tracing::info!("Applied migration 020_unsubscribe");
    }

    if current_version < 21 {
        conn.execute_batch(MIGRATION_021)?;
        tracing::info!("Applied migration 021_needs_reply");
    }

    if current_version < 22 {
        conn.execute_batch(MIGRATION_022)?;
        tracing::info!("Applied migration 022_contact_topics_cache");
    }

    if current_version < 23 {
        conn.execute_batch(MIGRATION_023)?;
        tracing::info!("Applied migration 023_thread_notes");
    }

    if current_version < 24 {
        conn.execute_batch(MIGRATION_024)?;
        tracing::info!("Applied migration 024_intent_detection");
    }

    if current_version < 25 {
        conn.execute_batch(MIGRATION_025)?;
        tracing::info!("Applied migration 025_deadlines");
    }

    if current_version < 26 {
        conn.execute_batch(MIGRATION_026)?;
        tracing::info!("Applied migration 026_vip_contacts");
    }

    if current_version < 27 {
        conn.execute_batch(MIGRATION_027)?;
        tracing::info!("Applied migration 027_followup_reminders");
    }

    if current_version < 28 {
        conn.execute_batch(MIGRATION_028)?;
        tracing::info!("Applied migration 028_relationship_priority");
    }

    if current_version < 29 {
        conn.execute_batch(MIGRATION_029)?;
        tracing::info!("Applied migration 029_social_engineering");
    }

    if current_version < 30 {
        conn.execute_batch(MIGRATION_030)?;
        tracing::info!("Applied migration 030_relationship_details");
    }

    if current_version < 31 {
        conn.execute_batch(MIGRATION_031)?;
        tracing::info!("Applied migration 031_draft_versions");
    }

    if current_version < 32 {
        conn.execute_batch(MIGRATION_032)?;
        tracing::info!("Applied migration 032_relationship_scores");
    }

    if current_version < 33 {
        conn.execute_batch(MIGRATION_033)?;
        tracing::info!("Applied migration 033_tracking_pixels");
    }

    if current_version < 34 {
        conn.execute_batch(MIGRATION_034)?;
        tracing::info!("Applied migration 034_archive_patterns");
    }

    if current_version < 35 {
        conn.execute_batch(MIGRATION_035)?;
        tracing::info!("Applied migration 035_newsletter_digests");
    }

    if current_version < 36 {
        conn.execute_batch(MIGRATION_036)?;
        tracing::info!("Applied migration 036_template_suggestions");
    }

    if current_version < 37 {
        conn.execute_batch(MIGRATION_037)?;
        tracing::info!("Applied migration 037_notification_routing");
    }

    if current_version < 38 {
        conn.execute_batch(MIGRATION_038)?;
        tracing::info!("Applied migration 038_followup_tracking");
    }

    if current_version < 39 {
        conn.execute_batch(MIGRATION_039)?;
        tracing::info!("Applied migration 039_effectiveness_scores");
    }

    if current_version < 40 {
        conn.execute_batch(MIGRATION_040)?;
        tracing::info!("Applied migration 040_webhooks");
    }

    if current_version < 41 {
        conn.execute_batch(MIGRATION_041)?;
        tracing::info!("Applied migration 041_extracted_data");
    }

    if current_version < 42 {
        conn.execute_batch(MIGRATION_042)?;
        tracing::info!("Applied migration 042_health_reports");
    }

    if current_version < 43 {
        conn.execute_batch(MIGRATION_043)?;
        tracing::info!("Applied migration 043_newsletter_feeds");
    }

    if current_version < 44 {
        conn.execute_batch(MIGRATION_044)?;
        tracing::info!("Applied migration 044_subscriptions");
    }

    if current_version < 45 {
        conn.execute_batch(MIGRATION_045)?;
        tracing::info!("Applied migration 045_analytics");
    }

    if current_version < 46 {
        conn.execute_batch(MIGRATION_046)?;
        tracing::info!("Applied migration 046_attachment_search");
    }

    if current_version < 47 {
        conn.execute_batch(MIGRATION_047)?;
        tracing::info!("Applied migration 047_thread_clusters");
    }

    if current_version < 48 {
        conn.execute_batch(MIGRATION_048)?;
        tracing::info!("Applied migration 048_phishing_detection");
    }

    if current_version < 49 {
        conn.execute_batch(MIGRATION_049)?;
        tracing::info!("Applied migration 049_contact_profiles");
    }

    if current_version < 50 {
        conn.execute_batch(MIGRATION_050)?;
        tracing::info!("Applied migration 050_mcp_server");
    }

    Ok(())
}
