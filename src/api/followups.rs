use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FollowupReminder {
    pub id: String,
    pub message_id: String,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub reason: String,
    pub suggested_date: String,
    pub urgency: String,
    pub status: String,
    pub snoozed_until: Option<String>,
    pub created_at: i64,
    pub dismissed_at: Option<i64>,
    pub acted_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListFollowupsResponse {
    pub reminders: Vec<FollowupReminder>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListFollowupsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SnoozeRequest {
    pub until: String,
}

#[derive(Debug, Serialize)]
pub struct ScanFollowupsResponse {
    pub scanned: usize,
    pub reminders_created: usize,
}

// ---------------------------------------------------------------------------
// AI prompt
// ---------------------------------------------------------------------------

const FOLLOWUP_SYSTEM_PROMPT: &str = r#"You are an email follow-up analyst. Given a sent email with no reply, determine if follow-up is needed.

Consider:
- Is this a question or request that expects a reply?
- How long has it been since the email was sent?
- Does the content suggest urgency or a deadline?
- Is this a cold outreach (lower follow-up priority) vs. ongoing thread (higher)?

For each email that needs follow-up, provide:
- reason: why follow-up is needed (1 sentence)
- suggested_date: when to follow up (YYYY-MM-DD). Today is {today}.
- urgency: low|normal|high|urgent

Respond with JSON only: {"needs_followup": true, "reason": "...", "suggested_date": "YYYY-MM-DD", "urgency": "..."}
If no follow-up needed: {"needs_followup": false}"#;

// ---------------------------------------------------------------------------
// AI response parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AiFollowupResponse {
    pub needs_followup: bool,
    pub reason: Option<String>,
    pub suggested_date: Option<String>,
    pub urgency: Option<String>,
}

/// Parse AI follow-up analysis response from JSON (potentially embedded in text).
pub fn parse_followup_response(response: &str) -> Option<AiFollowupResponse> {
    // Try direct parse first
    if let Ok(parsed) = serde_json::from_str::<AiFollowupResponse>(response) {
        return Some(parsed);
    }

    // Try extracting JSON from response text
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if let Ok(parsed) = serde_json::from_str::<AiFollowupResponse>(&response[start..=end]) {
                return Some(parsed);
            }
        }
    }

    None
}

/// Validate urgency value, defaulting to "normal" for invalid values.
pub fn validate_urgency(urgency: &str) -> &str {
    match urgency {
        "low" | "normal" | "high" | "urgent" => urgency,
        _ => "normal",
    }
}

/// Validate date format (YYYY-MM-DD). Returns true if valid.
pub fn validate_date(date: &str) -> bool {
    if date.len() != 10 {
        return false;
    }
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}

// ---------------------------------------------------------------------------
// GET /api/followups — list active reminders
// ---------------------------------------------------------------------------

pub async fn list_followups(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListFollowupsQuery>,
) -> Result<Json<ListFollowupsResponse>, StatusCode> {
    let limit = params.limit.unwrap_or(20).min(100);
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    let (sql, query_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(ref status) = params.status {
            (
                "SELECT r.id, r.message_id, r.thread_id, m.subject, r.reason, r.suggested_date,
                        r.urgency, r.status, r.snoozed_until, r.created_at, r.dismissed_at, r.acted_at
                 FROM followup_reminders r
                 LEFT JOIN messages m ON m.message_id = r.message_id
                 WHERE r.status = ?1
                 ORDER BY r.suggested_date ASC
                 LIMIT ?2".to_string(),
                vec![
                    Box::new(status.clone()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                ],
            )
        } else {
            // Default: pending + snoozed-and-due
            (
                "SELECT r.id, r.message_id, r.thread_id, m.subject, r.reason, r.suggested_date,
                        r.urgency, r.status, r.snoozed_until, r.created_at, r.dismissed_at, r.acted_at
                 FROM followup_reminders r
                 LEFT JOIN messages m ON m.message_id = r.message_id
                 WHERE r.status = 'pending'
                    OR (r.status = 'snoozed' AND r.snoozed_until <= ?1)
                 ORDER BY r.suggested_date ASC
                 LIMIT ?2".to_string(),
                vec![
                    Box::new(today) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                ],
            )
        };

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Failed to prepare followups query: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        query_params.iter().map(|p| p.as_ref()).collect();

    let reminders: Vec<FollowupReminder> = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(FollowupReminder {
                id: row.get(0)?,
                message_id: row.get(1)?,
                thread_id: row.get(2)?,
                subject: row.get(3)?,
                reason: row.get(4)?,
                suggested_date: row.get(5)?,
                urgency: row.get(6)?,
                status: row.get(7)?,
                snoozed_until: row.get(8)?,
                created_at: row.get(9)?,
                dismissed_at: row.get(10)?,
                acted_at: row.get(11)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query followups: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    let total = reminders.len();
    Ok(Json(ListFollowupsResponse { reminders, total }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/scan-followups — AI scan for follow-up candidates
// ---------------------------------------------------------------------------

pub async fn scan_followups(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScanFollowupsResponse>, StatusCode> {
    // Check AI is enabled
    let candidates = {
        let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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

        // Find sent emails from last 7 days with no reply in the same thread
        let seven_days_ago = chrono::Local::now().timestamp() - (7 * 86400);

        let mut stmt = conn
            .prepare(
                "SELECT m.message_id, m.thread_id, m.subject, m.from_address, m.snippet, m.date
                 FROM messages m
                 WHERE m.folder = 'Sent'
                   AND m.is_deleted = 0
                   AND m.date >= ?1
                   AND m.message_id IS NOT NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM messages m2
                       WHERE m2.thread_id = m.thread_id
                         AND m2.date > m.date
                         AND m2.folder != 'Sent'
                         AND m2.is_deleted = 0
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM followup_reminders r
                       WHERE r.message_id = m.message_id
                         AND r.status IN ('pending', 'snoozed')
                   )
                 ORDER BY m.date DESC
                 LIMIT 50",
            )
            .map_err(|e| {
                tracing::error!("Failed to prepare scan query: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let rows: Vec<(String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>)> = stmt
            .query_map(rusqlite::params![seven_days_ago], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(|e| {
                tracing::error!("Failed to query sent emails: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .filter_map(|r| r.ok())
            .collect();

        rows
    };

    let scanned = candidates.len();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let system_prompt = FOLLOWUP_SYSTEM_PROMPT.replace("{today}", &today);
    let mut reminders_created = 0usize;

    for (message_id, thread_id, subject, _from_addr, snippet, date) in &candidates {
        let days_ago = date
            .map(|d| (chrono::Local::now().timestamp() - d) / 86400)
            .unwrap_or(0);

        let prompt = format!(
            "Subject: {}\nSent {} days ago\nSnippet: {}\nThread: {}",
            subject.as_deref().unwrap_or("(no subject)"),
            days_ago,
            snippet.as_deref().unwrap_or(""),
            if thread_id.is_some() { "ongoing thread" } else { "standalone email" }
        );

        let ai_result = state
            .providers
            .generate(&prompt, Some(&system_prompt))
            .await;

        if let Some(response_text) = ai_result {
            if let Some(parsed) = parse_followup_response(&response_text) {
                if parsed.needs_followup {
                    let reason = parsed.reason.unwrap_or_else(|| "Follow-up recommended".to_string());
                    let suggested_date = parsed.suggested_date.unwrap_or_else(|| today.clone());
                    let urgency = parsed.urgency.as_deref().unwrap_or("normal");
                    let urgency = validate_urgency(urgency);

                    // Validate date format
                    let final_date = if validate_date(&suggested_date) {
                        suggested_date
                    } else {
                        today.clone()
                    };

                    let id = uuid::Uuid::new_v4().to_string();

                    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    let inserted = conn
                        .execute(
                            "INSERT OR IGNORE INTO followup_reminders (id, message_id, thread_id, reason, suggested_date, urgency)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![id, message_id, thread_id, reason, final_date, urgency],
                        )
                        .unwrap_or(0);

                    if inserted > 0 {
                        reminders_created += 1;
                    }
                }
            }
        }
    }

    Ok(Json(ScanFollowupsResponse {
        scanned,
        reminders_created,
    }))
}

// ---------------------------------------------------------------------------
// PUT /api/followups/{id}/snooze
// ---------------------------------------------------------------------------

pub async fn snooze_followup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<SnoozeRequest>,
) -> Result<StatusCode, StatusCode> {
    if !validate_date(&body.until) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let updated = conn
        .execute(
            "UPDATE followup_reminders SET status = 'snoozed', snoozed_until = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
            rusqlite::params![body.until, id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// PUT /api/followups/{id}/dismiss
// ---------------------------------------------------------------------------

pub async fn dismiss_followup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = chrono::Utc::now().timestamp();

    let updated = conn
        .execute(
            "UPDATE followup_reminders SET status = 'dismissed', dismissed_at = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
            rusqlite::params![now, id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// PUT /api/followups/{id}/acted
// ---------------------------------------------------------------------------

pub async fn mark_acted(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = chrono::Utc::now().timestamp();

    let updated = conn
        .execute(
            "UPDATE followup_reminders SET status = 'acted', acted_at = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
            rusqlite::params![now, id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if updated == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};

    fn setup_db() -> (crate::db::DbPool, String) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create the followup_reminders table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS followup_reminders (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                thread_id TEXT,
                reason TEXT NOT NULL,
                suggested_date TEXT NOT NULL,
                urgency TEXT NOT NULL DEFAULT 'normal',
                status TEXT NOT NULL DEFAULT 'pending',
                snoozed_until TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                dismissed_at INTEGER,
                acted_at INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_followup_status ON followup_reminders(status);
            CREATE INDEX IF NOT EXISTS idx_followup_date ON followup_reminders(suggested_date);
            CREATE INDEX IF NOT EXISTS idx_followup_message ON followup_reminders(message_id);",
        )
        .unwrap();

        // Create a test account
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "followup-test@example.com".to_string(),
            display_name: Some("Followup Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("followup-test@example.com".to_string()),
            password: None,
        };
        let account = Account::create(&conn, &input);

        (pool, account.id)
    }

    fn insert_sent_message(
        conn: &rusqlite::Connection,
        account_id: &str,
        message_id: &str,
        thread_id: Option<&str>,
        subject: &str,
        date: i64,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, message_id, thread_id, folder, from_address, from_name, subject, snippet, date, is_read, is_starred, has_attachments, is_draft)
             VALUES (?1, ?2, ?3, ?4, 'Sent', 'me@example.com', 'Me', ?5, 'snippet text', ?6, 1, 0, 0, 0)",
            rusqlite::params![id, account_id, message_id, thread_id, subject, date],
        )
        .unwrap();
        id
    }

    fn insert_reminder(
        conn: &rusqlite::Connection,
        id: &str,
        message_id: &str,
        thread_id: Option<&str>,
        reason: &str,
        suggested_date: &str,
        urgency: &str,
        status: &str,
        snoozed_until: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO followup_reminders (id, message_id, thread_id, reason, suggested_date, urgency, status, snoozed_until)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![id, message_id, thread_id, reason, suggested_date, urgency, status, snoozed_until],
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // AI response parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_followup_needs_followup() {
        let response = r#"{"needs_followup": true, "reason": "No reply after 3 days", "suggested_date": "2026-03-20", "urgency": "high"}"#;
        let parsed = parse_followup_response(response).unwrap();
        assert!(parsed.needs_followup);
        assert_eq!(parsed.reason.as_deref(), Some("No reply after 3 days"));
        assert_eq!(parsed.suggested_date.as_deref(), Some("2026-03-20"));
        assert_eq!(parsed.urgency.as_deref(), Some("high"));
    }

    #[test]
    fn test_parse_followup_no_followup() {
        let response = r#"{"needs_followup": false}"#;
        let parsed = parse_followup_response(response).unwrap();
        assert!(!parsed.needs_followup);
        assert!(parsed.reason.is_none());
    }

    #[test]
    fn test_parse_followup_embedded_json() {
        let response = r#"Based on my analysis:
{"needs_followup": true, "reason": "Action item pending", "suggested_date": "2026-03-18", "urgency": "normal"}
That's my recommendation."#;
        let parsed = parse_followup_response(response).unwrap();
        assert!(parsed.needs_followup);
        assert_eq!(parsed.reason.as_deref(), Some("Action item pending"));
    }

    #[test]
    fn test_parse_followup_malformed() {
        let response = "I'm not sure about this email.";
        let parsed = parse_followup_response(response);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_validate_urgency_valid() {
        assert_eq!(validate_urgency("low"), "low");
        assert_eq!(validate_urgency("normal"), "normal");
        assert_eq!(validate_urgency("high"), "high");
        assert_eq!(validate_urgency("urgent"), "urgent");
    }

    #[test]
    fn test_validate_urgency_invalid() {
        assert_eq!(validate_urgency("critical"), "normal");
        assert_eq!(validate_urgency(""), "normal");
        assert_eq!(validate_urgency("URGENT"), "normal");
    }

    #[test]
    fn test_validate_date_valid() {
        assert!(validate_date("2026-03-20"));
        assert!(validate_date("2026-12-31"));
    }

    #[test]
    fn test_validate_date_invalid() {
        assert!(!validate_date("2026/03/20"));
        assert!(!validate_date("not-a-date"));
        assert!(!validate_date("20260320"));
        assert!(!validate_date("2026-13-01"));
        assert!(!validate_date(""));
    }

    // -----------------------------------------------------------------------
    // DB operation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_pending_reminders() {
        let (pool, account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_sent_message(&conn, &account_id, "<msg1@test.com>", Some("thread-1"), "Hello", 1700000000);
        insert_reminder(&conn, "r1", "<msg1@test.com>", Some("thread-1"), "No reply after 3 days", "2026-03-20", "high", "pending", None);
        insert_reminder(&conn, "r2", "<msg1@test.com>", Some("thread-1"), "Dismissed one", "2026-03-18", "normal", "dismissed", None);

        // Query pending reminders
        let mut stmt = conn.prepare(
            "SELECT r.id, r.message_id, r.thread_id, m.subject, r.reason, r.suggested_date,
                    r.urgency, r.status, r.snoozed_until, r.created_at, r.dismissed_at, r.acted_at
             FROM followup_reminders r
             LEFT JOIN messages m ON m.message_id = r.message_id
             WHERE r.status = 'pending'
             ORDER BY r.suggested_date ASC"
        ).unwrap();

        let reminders: Vec<FollowupReminder> = stmt
            .query_map([], |row| {
                Ok(FollowupReminder {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    thread_id: row.get(2)?,
                    subject: row.get(3)?,
                    reason: row.get(4)?,
                    suggested_date: row.get(5)?,
                    urgency: row.get(6)?,
                    status: row.get(7)?,
                    snoozed_until: row.get(8)?,
                    created_at: row.get(9)?,
                    dismissed_at: row.get(10)?,
                    acted_at: row.get(11)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].id, "r1");
        assert_eq!(reminders[0].urgency, "high");
        assert_eq!(reminders[0].subject.as_deref(), Some("Hello"));
    }

    #[test]
    fn test_snooze_updates_status_and_date() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_reminder(&conn, "r3", "<msg@test.com>", None, "Needs reply", "2026-03-15", "normal", "pending", None);

        let updated = conn
            .execute(
                "UPDATE followup_reminders SET status = 'snoozed', snoozed_until = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
                rusqlite::params!["2026-03-25", "r3"],
            )
            .unwrap();
        assert_eq!(updated, 1);

        let (status, snoozed): (String, Option<String>) = conn
            .query_row(
                "SELECT status, snoozed_until FROM followup_reminders WHERE id = 'r3'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "snoozed");
        assert_eq!(snoozed.as_deref(), Some("2026-03-25"));
    }

    #[test]
    fn test_dismiss_marks_dismissed() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_reminder(&conn, "r4", "<msg@test.com>", None, "Needs reply", "2026-03-15", "normal", "pending", None);

        let now = chrono::Utc::now().timestamp();
        let updated = conn
            .execute(
                "UPDATE followup_reminders SET status = 'dismissed', dismissed_at = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
                rusqlite::params![now, "r4"],
            )
            .unwrap();
        assert_eq!(updated, 1);

        let (status, dismissed_at): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, dismissed_at FROM followup_reminders WHERE id = 'r4'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "dismissed");
        assert!(dismissed_at.is_some());
    }

    #[test]
    fn test_mark_acted() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_reminder(&conn, "r5", "<msg@test.com>", None, "Needs reply", "2026-03-15", "normal", "pending", None);

        let now = chrono::Utc::now().timestamp();
        let updated = conn
            .execute(
                "UPDATE followup_reminders SET status = 'acted', acted_at = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
                rusqlite::params![now, "r5"],
            )
            .unwrap();
        assert_eq!(updated, 1);

        let (status, acted_at): (String, Option<i64>) = conn
            .query_row(
                "SELECT status, acted_at FROM followup_reminders WHERE id = 'r5'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "acted");
        assert!(acted_at.is_some());
    }

    #[test]
    fn test_snoozed_and_due_logic() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let future = "2099-12-31";

        // Snoozed until today (due)
        insert_reminder(&conn, "r6", "<msg@test.com>", None, "Due today", "2026-03-10", "normal", "snoozed", Some(&today));
        // Snoozed until future (not due)
        insert_reminder(&conn, "r7", "<msg@test.com>", None, "Future snooze", "2026-03-10", "normal", "snoozed", Some(future));
        // Pending (always shown)
        insert_reminder(&conn, "r8", "<msg@test.com>", None, "Pending one", "2026-03-12", "high", "pending", None);

        let mut stmt = conn.prepare(
            "SELECT id FROM followup_reminders
             WHERE status = 'pending'
                OR (status = 'snoozed' AND snoozed_until <= ?1)
             ORDER BY suggested_date ASC"
        ).unwrap();

        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![today], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"r6".to_string()));
        assert!(ids.contains(&"r8".to_string()));
        assert!(!ids.contains(&"r7".to_string()));
    }

    #[test]
    fn test_scan_identifies_unreplied_sent_emails() {
        let (pool, account_id) = setup_db();
        let conn = pool.get().unwrap();

        let now = chrono::Local::now().timestamp();
        let three_days_ago = now - (3 * 86400);

        // Sent email with no reply (should be candidate)
        insert_sent_message(&conn, &account_id, "<sent1@test.com>", Some("thread-1"), "Need your input", three_days_ago);

        // Sent email with a reply (should NOT be candidate)
        insert_sent_message(&conn, &account_id, "<sent2@test.com>", Some("thread-2"), "Already replied", three_days_ago);
        // Insert reply in same thread
        let reply_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, message_id, thread_id, folder, from_address, subject, date, is_read, is_starred, has_attachments, is_draft)
             VALUES (?1, ?2, '<reply2@test.com>', 'thread-2', 'INBOX', 'other@test.com', 'Re: Already replied', ?3, 0, 0, 0, 0)",
            rusqlite::params![reply_id, account_id, three_days_ago + 3600],
        ).unwrap();

        // Query unreplied sent emails
        let seven_days_ago = now - (7 * 86400);
        let mut stmt = conn.prepare(
            "SELECT m.message_id, m.subject
             FROM messages m
             WHERE m.folder = 'Sent'
               AND m.is_deleted = 0
               AND m.date >= ?1
               AND m.message_id IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1 FROM messages m2
                   WHERE m2.thread_id = m.thread_id
                     AND m2.date > m.date
                     AND m2.folder != 'Sent'
                     AND m2.is_deleted = 0
               )"
        ).unwrap();

        let candidates: Vec<(String, Option<String>)> = stmt
            .query_map(rusqlite::params![seven_days_ago], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "<sent1@test.com>");
    }

    #[test]
    fn test_cannot_snooze_dismissed_reminder() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_reminder(&conn, "r9", "<msg@test.com>", None, "Already dismissed", "2026-03-15", "normal", "dismissed", None);

        let updated = conn
            .execute(
                "UPDATE followup_reminders SET status = 'snoozed', snoozed_until = ?1 WHERE id = ?2 AND status IN ('pending', 'snoozed')",
                rusqlite::params!["2026-03-25", "r9"],
            )
            .unwrap();
        assert_eq!(updated, 0);
    }

    #[test]
    fn test_listing_filters_by_status() {
        let (pool, _account_id) = setup_db();
        let conn = pool.get().unwrap();

        insert_reminder(&conn, "ra", "<msg@test.com>", None, "Pending", "2026-03-15", "normal", "pending", None);
        insert_reminder(&conn, "rb", "<msg@test.com>", None, "Dismissed", "2026-03-15", "low", "dismissed", None);
        insert_reminder(&conn, "rc", "<msg@test.com>", None, "Acted", "2026-03-15", "high", "acted", None);

        // Filter by dismissed
        let mut stmt = conn.prepare(
            "SELECT id FROM followup_reminders WHERE status = ?1"
        ).unwrap();
        let dismissed: Vec<String> = stmt
            .query_map(rusqlite::params!["dismissed"], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(dismissed.len(), 1);
        assert_eq!(dismissed[0], "rb");

        // Filter by acted
        let acted: Vec<String> = stmt
            .query_map(rusqlite::params!["acted"], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(acted.len(), 1);
        assert_eq!(acted[0], "rc");
    }
}
