use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::writing_style;
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDraft {
    pub id: String,
    pub message_id: String,
    pub account_id: String,
    pub pattern_id: Option<String>,
    pub draft_body: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct CheckDraftResponse {
    pub draft: Option<AutoDraft>,
}

#[derive(Debug, Serialize)]
pub struct GenerateDraftResponse {
    pub draft: AutoDraft,
}

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct FeedbackResponse {
    pub updated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDraftConfig {
    pub enabled: bool,
    pub sensitivity: String, // conservative, balanced, aggressive
}

// ---------------------------------------------------------------------------
// GET /api/auto-draft/{message_id}
// ---------------------------------------------------------------------------

pub async fn check_auto_draft(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<CheckDraftResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let draft = conn
        .query_row(
            "SELECT id, message_id, account_id, pattern_id, draft_body, status, created_at
             FROM auto_drafts
             WHERE message_id = ?1 AND status = 'pending'
             ORDER BY created_at DESC
             LIMIT 1",
            rusqlite::params![message_id],
            |row| {
                Ok(AutoDraft {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    account_id: row.get(2)?,
                    pattern_id: row.get(3)?,
                    draft_body: row.get(4)?,
                    status: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
        .ok();

    Ok(Json(CheckDraftResponse { draft }))
}

// ---------------------------------------------------------------------------
// POST /api/auto-draft/generate/{message_id}
// ---------------------------------------------------------------------------

pub async fn generate_auto_draft(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<GenerateDraftResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check AI enabled
    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Check auto-draft is enabled
    let ad_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auto_draft_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ad_enabled != "true" {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Fetch the incoming message
    let (account_id, from_address, subject, body_text): (String, String, String, String) = conn
        .query_row(
            "SELECT account_id, COALESCE(from_address, ''), COALESCE(subject, ''), COALESCE(body_text, '')
             FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    // Check for existing pending draft
    let existing: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM auto_drafts WHERE message_id = ?1 AND status = 'pending')",
            rusqlite::params![message_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if existing {
        // Return existing draft
        let draft = conn
            .query_row(
                "SELECT id, message_id, account_id, pattern_id, draft_body, status, created_at
                 FROM auto_drafts WHERE message_id = ?1 AND status = 'pending' LIMIT 1",
                rusqlite::params![message_id],
                |row| {
                    Ok(AutoDraft {
                        id: row.get(0)?,
                        message_id: row.get(1)?,
                        account_id: row.get(2)?,
                        pattern_id: row.get(3)?,
                        draft_body: row.get(4)?,
                        status: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        return Ok(Json(GenerateDraftResponse { draft }));
    }

    // Get confidence threshold from sensitivity setting
    let sensitivity = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auto_draft_sensitivity'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "balanced".to_string());

    let confidence_threshold = match sensitivity.as_str() {
        "conservative" => 0.8,
        "aggressive" => 0.4,
        _ => 0.6, // balanced
    };

    // Try pattern matching first
    let sender_domain = from_address
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_lowercase();

    let matched_pattern = find_matching_pattern(&conn, &account_id, &sender_domain, &subject, confidence_threshold);

    let (draft_body, pattern_id) = if let Some((pid, template)) = matched_pattern {
        // Update pattern match count
        conn.execute(
            "UPDATE auto_draft_patterns SET match_count = match_count + 1, last_matched_at = unixepoch(), updated_at = unixepoch() WHERE id = ?1",
            rusqlite::params![pid],
        )
        .ok();
        (template, Some(pid))
    } else {
        // Generate via AI
        let body_truncated: String = body_text.chars().take(2000).collect();

        // Get writing style
        let style_snippet = writing_style::build_style_prompt(&conn, &account_id);

        let mut system = String::from(
            "You are an email reply assistant. Generate a professional, contextual reply to the email. \
             Return ONLY the reply body text, no subject line, no greetings preamble instructions. \
             Write a complete, ready-to-send reply."
        );

        if let Some(style) = style_snippet {
            system.push_str("\n\n");
            system.push_str(&style);
        }

        let prompt = format!(
            "Write a reply to this email:\n\nFrom: {}\nSubject: {}\n\n{}",
            from_address, subject, body_truncated
        );

        let generated = state
            .providers
            .generate(&prompt, Some(&system))
            .await
            .ok_or(StatusCode::BAD_GATEWAY)?;

        (generated, None)
    };

    // Store the auto-draft
    let draft_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO auto_drafts (id, message_id, account_id, pattern_id, draft_body, status)
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
        rusqlite::params![draft_id, message_id, account_id, pattern_id, draft_body],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let draft = AutoDraft {
        id: draft_id,
        message_id,
        account_id,
        pattern_id,
        draft_body,
        status: "pending".to_string(),
        created_at: chrono::Utc::now().timestamp(),
    };

    Ok(Json(GenerateDraftResponse { draft }))
}

// ---------------------------------------------------------------------------
// POST /api/auto-draft/{draft_id}/feedback
// ---------------------------------------------------------------------------

pub async fn auto_draft_feedback(
    State(state): State<Arc<AppState>>,
    Path(draft_id): Path<String>,
    Json(input): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, StatusCode> {
    let valid_actions = ["used", "dismissed", "edited"];
    if !valid_actions.contains(&input.action.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update draft status
    let updated = conn
        .execute(
            "UPDATE auto_drafts SET status = ?1 WHERE id = ?2 AND status = 'pending'",
            rusqlite::params![input.action, draft_id],
        )
        .unwrap_or(0);

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    // Adjust pattern confidence if there's a pattern_id
    let pattern_id: Option<String> = conn
        .query_row(
            "SELECT pattern_id FROM auto_drafts WHERE id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    if let Some(pid) = pattern_id {
        let adjustment = match input.action.as_str() {
            "used" => 0.05,
            "dismissed" => -0.1,
            _ => 0.0, // edited = no change
        };

        if adjustment != 0.0 {
            conn.execute(
                "UPDATE auto_draft_patterns SET success_rate = MIN(1.0, MAX(0.0, success_rate + ?1)), updated_at = unixepoch() WHERE id = ?2",
                rusqlite::params![adjustment, pid],
            )
            .ok();
        }
    }

    Ok(Json(FeedbackResponse { updated: true }))
}

// ---------------------------------------------------------------------------
// GET /api/config/auto-draft
// ---------------------------------------------------------------------------

pub async fn get_auto_draft_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AutoDraftConfig>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auto_draft_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string())
        == "true";

    let sensitivity = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auto_draft_sensitivity'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "balanced".to_string());

    Ok(Json(AutoDraftConfig {
        enabled,
        sensitivity,
    }))
}

// ---------------------------------------------------------------------------
// PUT /api/config/auto-draft
// ---------------------------------------------------------------------------

pub async fn set_auto_draft_config(
    State(state): State<Arc<AppState>>,
    Json(input): Json<AutoDraftConfig>,
) -> Result<Json<AutoDraftConfig>, StatusCode> {
    let valid_sensitivities = ["conservative", "balanced", "aggressive"];
    if !valid_sensitivities.contains(&input.sensitivity.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('auto_draft_enabled', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![if input.enabled { "true" } else { "false" }],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('auto_draft_sensitivity', ?1) ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![input.sensitivity],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(input))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_matching_pattern(
    conn: &rusqlite::Connection,
    account_id: &str,
    sender_domain: &str,
    subject: &str,
    confidence_threshold: f64,
) -> Option<(String, String)> {
    // Build a pattern hash from sender domain + subject keywords
    let subject_lower = subject.to_lowercase();
    let keywords: Vec<&str> = subject_lower
        .split_whitespace()
        .filter(|w| w.len() > 3)
        .take(5)
        .collect();

    // Try exact pattern hash match first
    let pattern_hash = format!("{}:{}", sender_domain, keywords.join(","));

    let result = conn
        .query_row(
            "SELECT id, template_body FROM auto_draft_patterns
             WHERE account_id = ?1 AND pattern_hash = ?2 AND success_rate >= ?3
             ORDER BY match_count DESC
             LIMIT 1",
            rusqlite::params![account_id, pattern_hash, confidence_threshold],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .ok();

    if result.is_some() {
        return result;
    }

    // Try domain-only match with high confidence
    let escaped_domain = sender_domain.replace('%', "\\%").replace('_', "\\_");
    conn.query_row(
        "SELECT id, template_body FROM auto_draft_patterns
         WHERE account_id = ?1 AND pattern_hash LIKE ?2 ESCAPE '\\' AND success_rate >= ?3
         ORDER BY match_count DESC
         LIMIT 1",
        rusqlite::params![account_id, format!("{}:%", escaped_domain), confidence_threshold + 0.1],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .ok()
}

/// Public pattern matching helper — callable from worker.
pub fn find_matching_pattern_for_worker(
    conn: &rusqlite::Connection,
    account_id: &str,
    sender_domain: &str,
    subject: &str,
    confidence_threshold: f64,
) -> Option<(String, String)> {
    find_matching_pattern(conn, account_id, sender_domain, subject, confidence_threshold)
}

/// Enqueue an auto-draft generation job for a message (called from sync/worker).
pub fn enqueue_auto_draft(conn: &rusqlite::Connection, message_id: &str) {
    // Check if auto-draft is enabled
    let enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'auto_draft_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if enabled != "true" {
        return;
    }

    // Dedup check
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM processing_jobs WHERE job_type = 'auto_draft' AND message_id = ?1 AND status IN ('pending','processing'))",
            rusqlite::params![message_id],
            |row| row.get(0),
        )
        .unwrap_or(true);

    if exists {
        return;
    }

    if let Err(e) = conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id) VALUES ('auto_draft', ?1)",
        rusqlite::params![message_id],
    ) {
        tracing::warn!("Failed to enqueue auto_draft: {e}");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS accounts (id TEXT PRIMARY KEY, email TEXT, is_active INTEGER DEFAULT 1);
             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT PRIMARY KEY,
                 account_id TEXT,
                 from_address TEXT,
                 subject TEXT,
                 body_text TEXT,
                 ai_status TEXT,
                 memories_status TEXT
             );
             CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS processing_jobs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 job_type TEXT NOT NULL,
                 message_id TEXT,
                 status TEXT NOT NULL DEFAULT 'pending',
                 attempts INTEGER NOT NULL DEFAULT 0,
                 max_attempts INTEGER NOT NULL DEFAULT 4,
                 payload TEXT,
                 error TEXT,
                 created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                 next_retry_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             CREATE TABLE IF NOT EXISTS auto_draft_patterns (
                 id TEXT PRIMARY KEY,
                 account_id TEXT NOT NULL,
                 pattern_hash TEXT NOT NULL,
                 trigger_description TEXT NOT NULL,
                 template_body TEXT NOT NULL,
                 match_count INTEGER DEFAULT 0,
                 success_rate REAL DEFAULT 0.5,
                 last_matched_at INTEGER,
                 created_at INTEGER DEFAULT (unixepoch()),
                 updated_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(account_id) REFERENCES accounts(id)
             );
             CREATE TABLE IF NOT EXISTS auto_drafts (
                 id TEXT PRIMARY KEY,
                 message_id TEXT NOT NULL,
                 account_id TEXT NOT NULL,
                 pattern_id TEXT,
                 draft_body TEXT NOT NULL,
                 status TEXT DEFAULT 'pending' CHECK(status IN ('pending', 'used', 'dismissed', 'edited')),
                 created_at INTEGER DEFAULT (unixepoch()),
                 FOREIGN KEY(message_id) REFERENCES messages(id),
                 FOREIGN KEY(account_id) REFERENCES accounts(id),
                 FOREIGN KEY(pattern_id) REFERENCES auto_draft_patterns(id)
             );
             CREATE INDEX IF NOT EXISTS idx_auto_drafts_message ON auto_drafts(message_id);
             CREATE INDEX IF NOT EXISTS idx_auto_draft_patterns_account ON auto_draft_patterns(account_id);
             INSERT INTO accounts (id, email) VALUES ('acc1', 'test@example.com');",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_find_matching_pattern_exact() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO auto_draft_patterns (id, account_id, pattern_hash, trigger_description, template_body, match_count, success_rate)
             VALUES ('p1', 'acc1', 'example.com:meeting,update', 'Weekly meeting update', 'Thanks for the update!', 5, 0.8)",
            [],
        )
        .unwrap();

        let result = find_matching_pattern(&conn, "acc1", "example.com", "Meeting Update for Today", 0.6);
        assert!(result.is_some());
        let (id, body) = result.unwrap();
        assert_eq!(id, "p1");
        assert_eq!(body, "Thanks for the update!");
    }

    #[test]
    fn test_find_matching_pattern_low_confidence() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO auto_draft_patterns (id, account_id, pattern_hash, trigger_description, template_body, match_count, success_rate)
             VALUES ('p1', 'acc1', 'example.com:meeting', 'Meeting', 'OK!', 1, 0.3)",
            [],
        )
        .unwrap();

        let result = find_matching_pattern(&conn, "acc1", "example.com", "Meeting", 0.6);
        assert!(result.is_none());
    }

    #[test]
    fn test_enqueue_auto_draft_disabled() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id, account_id) VALUES ('msg1', 'acc1')", []).unwrap();

        enqueue_auto_draft(&conn, "msg1");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'auto_draft'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0); // Should not enqueue when disabled
    }

    #[test]
    fn test_enqueue_auto_draft_enabled() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id, account_id) VALUES ('msg1', 'acc1')", []).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('auto_draft_enabled', 'true')",
            [],
        )
        .unwrap();

        enqueue_auto_draft(&conn, "msg1");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'auto_draft'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_enqueue_auto_draft_dedup() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id, account_id) VALUES ('msg1', 'acc1')", []).unwrap();
        conn.execute(
            "INSERT INTO config (key, value) VALUES ('auto_draft_enabled', 'true')",
            [],
        )
        .unwrap();

        enqueue_auto_draft(&conn, "msg1");
        enqueue_auto_draft(&conn, "msg1"); // duplicate

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM processing_jobs WHERE job_type = 'auto_draft'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_feedback_confidence_adjustment() {
        let conn = setup_db();
        conn.execute("INSERT INTO messages (id, account_id) VALUES ('msg1', 'acc1')", []).unwrap();
        conn.execute(
            "INSERT INTO auto_draft_patterns (id, account_id, pattern_hash, trigger_description, template_body, success_rate)
             VALUES ('p1', 'acc1', 'test:hash', 'Test', 'Reply body', 0.5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO auto_drafts (id, message_id, account_id, pattern_id, draft_body, status)
             VALUES ('d1', 'msg1', 'acc1', 'p1', 'Reply body', 'pending')",
            [],
        )
        .unwrap();

        // Simulate "used" feedback
        conn.execute("UPDATE auto_drafts SET status = 'used' WHERE id = 'd1'", []).unwrap();
        conn.execute(
            "UPDATE auto_draft_patterns SET success_rate = MIN(1.0, MAX(0.0, success_rate + 0.05)) WHERE id = 'p1'",
            [],
        )
        .unwrap();

        let rate: f64 = conn
            .query_row("SELECT success_rate FROM auto_draft_patterns WHERE id = 'p1'", [], |row| row.get(0))
            .unwrap();
        assert!((rate - 0.55).abs() < 0.001);
    }
}
