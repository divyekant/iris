use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deadline {
    pub id: String,
    pub message_id: String,
    pub thread_id: Option<String>,
    pub description: String,
    pub deadline_date: String,
    pub deadline_source: String,
    pub is_explicit: bool,
    pub completed: bool,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct DeadlinesResponse {
    pub deadlines: Vec<Deadline>,
}

#[derive(Debug, Deserialize)]
pub struct ListDeadlinesQuery {
    pub days: Option<i64>,
    pub include_completed: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    pub message_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AiDeadline {
    pub(crate) description: String,
    pub(crate) deadline_date: String,
    pub(crate) deadline_source: String,
    pub(crate) is_explicit: bool,
}

#[derive(Debug, Deserialize)]
struct AiDeadlinesResponse {
    deadlines: Vec<AiDeadline>,
}

// ---------------------------------------------------------------------------
// AI Prompt
// ---------------------------------------------------------------------------

const DEADLINE_SYSTEM_PROMPT_TEMPLATE: &str = r#"You are a deadline extraction specialist. Given an email, identify any deadlines, due dates, or time-sensitive commitments mentioned.

For each deadline found, extract:
- description: what needs to be done by the deadline
- deadline_date: the date in YYYY-MM-DD format (use today's date context: {today})
- deadline_source: the exact phrase from the email mentioning the deadline
- is_explicit: true if a specific date/day was mentioned, false if vague (e.g., "soon", "ASAP")

For relative dates like "by Friday" or "next week", calculate the actual date based on today ({today}).

Respond with JSON only: {"deadlines": [...]}
If no deadlines found, respond: {"deadlines": []}"#;

/// Build the system prompt with today's date injected.
pub fn build_deadline_system_prompt(today: &str) -> String {
    DEADLINE_SYSTEM_PROMPT_TEMPLATE.replace("{today}", today)
}

/// Build the user prompt from message content.
pub fn build_deadline_user_prompt(subject: &str, body: &str) -> String {
    let body_truncated: String = if body.chars().count() > 3000 {
        let mut s: String = body.chars().take(3000).collect();
        s.push_str("...");
        s
    } else {
        body.to_string()
    };

    format!("Subject: {}\n\n{}", subject, body_truncated)
}

/// Parse the AI response JSON into a list of deadline structs.
/// Returns an empty vec on malformed input.
pub(crate) fn parse_deadline_response(raw: &str) -> Vec<AiDeadline> {
    // Try to extract JSON from the response (may have markdown fences)
    let json_str = extract_json(raw);
    serde_json::from_str::<AiDeadlinesResponse>(json_str)
        .map(|r| r.deadlines)
        .unwrap_or_default()
}

/// Extract JSON from potentially markdown-fenced response.
fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/deadlines — list upcoming deadlines
pub async fn list_deadlines(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListDeadlinesQuery>,
) -> Result<Json<DeadlinesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let days = params.days.unwrap_or(7);
    let include_completed = params.include_completed.unwrap_or(false);

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let end_date = (chrono::Local::now() + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();

    let deadlines = if include_completed {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, thread_id, description, deadline_date,
                        deadline_source, is_explicit, completed, created_at
                 FROM deadlines
                 WHERE deadline_date <= ?2
                 ORDER BY deadline_date ASC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(rusqlite::params![today, end_date], deadline_from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, thread_id, description, deadline_date,
                        deadline_source, is_explicit, completed, created_at
                 FROM deadlines
                 WHERE completed = 0 AND deadline_date <= ?1
                 ORDER BY deadline_date ASC",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(rusqlite::params![end_date], deadline_from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(Json(DeadlinesResponse { deadlines }))
}

/// GET /api/threads/{id}/deadlines — get deadlines for a thread
pub async fn thread_deadlines(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<DeadlinesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, message_id, thread_id, description, deadline_date,
                    deadline_source, is_explicit, completed, created_at
             FROM deadlines
             WHERE thread_id = ?1
             ORDER BY deadline_date ASC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let deadlines: Vec<Deadline> = stmt
        .query_map(rusqlite::params![thread_id], deadline_from_row)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(DeadlinesResponse { deadlines }))
}

/// POST /api/ai/extract-deadlines — on-demand extraction
pub async fn extract_deadlines(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ExtractRequest>,
) -> Result<Json<DeadlinesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check AI is enabled
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

    // Fetch the message
    let message = MessageDetail::get_by_id(&conn, &input.message_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let subject = message.subject.as_deref().unwrap_or("(no subject)");
    let body = message.body_text.as_deref().unwrap_or("");

    if body.is_empty() && subject == "(no subject)" {
        return Ok(Json(DeadlinesResponse { deadlines: vec![] }));
    }

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let system_prompt = build_deadline_system_prompt(&today);
    let user_prompt = build_deadline_user_prompt(subject, body);

    let raw_response = state
        .providers
        .generate(&user_prompt, Some(&system_prompt))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let extracted = parse_deadline_response(&raw_response);

    // Store extracted deadlines in the database
    let mut deadlines = Vec::new();
    for d in &extracted {
        let id = uuid::Uuid::new_v4().to_string();
        let result = conn.execute(
            "INSERT OR IGNORE INTO deadlines (id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id,
                input.message_id,
                message.thread_id,
                d.description,
                d.deadline_date,
                d.deadline_source,
                d.is_explicit as i32,
            ],
        );

        if let Ok(rows) = result {
            if rows > 0 {
                deadlines.push(Deadline {
                    id,
                    message_id: input.message_id.clone(),
                    thread_id: message.thread_id.clone(),
                    description: d.description.clone(),
                    deadline_date: d.deadline_date.clone(),
                    deadline_source: d.deadline_source.clone(),
                    is_explicit: d.is_explicit,
                    completed: false,
                    created_at: chrono::Local::now().timestamp(),
                });
            }
        }
    }

    // If some were duplicates, also return the existing ones for this message
    if deadlines.len() < extracted.len() {
        let mut stmt = conn
            .prepare(
                "SELECT id, message_id, thread_id, description, deadline_date,
                        deadline_source, is_explicit, completed, created_at
                 FROM deadlines WHERE message_id = ?1",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        deadlines = stmt
            .query_map(rusqlite::params![input.message_id], deadline_from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();
    }

    Ok(Json(DeadlinesResponse { deadlines }))
}

/// PUT /api/deadlines/{id}/complete — mark as completed
pub async fn complete_deadline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = conn
        .execute(
            "UPDATE deadlines SET completed = 1 WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// DELETE /api/deadlines/{id} — dismiss/delete a deadline
pub async fn delete_deadline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = conn
        .execute(
            "DELETE FROM deadlines WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn deadline_from_row(row: &rusqlite::Row) -> rusqlite::Result<Deadline> {
    Ok(Deadline {
        id: row.get(0)?,
        message_id: row.get(1)?,
        thread_id: row.get(2)?,
        description: row.get(3)?,
        deadline_date: row.get(4)?,
        deadline_source: row.get(5)?,
        is_explicit: row.get::<_, i32>(6)? != 0,
        completed: row.get::<_, i32>(7)? != 0,
        created_at: row.get(8)?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    #[test]
    fn test_build_deadline_system_prompt_injects_date() {
        let prompt = build_deadline_system_prompt("2026-03-15");
        assert!(prompt.contains("2026-03-15"));
        assert!(prompt.contains("deadline extraction specialist"));
        // Should replace both occurrences of {today}
        assert!(!prompt.contains("{today}"));
    }

    #[test]
    fn test_build_deadline_user_prompt() {
        let prompt = build_deadline_user_prompt("Meeting Friday", "Please submit by March 20");
        assert!(prompt.contains("Subject: Meeting Friday"));
        assert!(prompt.contains("Please submit by March 20"));
    }

    #[test]
    fn test_build_deadline_user_prompt_truncates_long_body() {
        let long_body = "x".repeat(5000);
        let prompt = build_deadline_user_prompt("Test", &long_body);
        // Should be truncated to 3000 chars + "..." + subject line
        assert!(prompt.len() < 3100);
        assert!(prompt.contains("..."));
    }

    #[test]
    fn test_parse_deadline_response_valid() {
        let json = r#"{"deadlines": [{"description": "Submit report", "deadline_date": "2026-03-20", "deadline_source": "by March 20", "is_explicit": true}]}"#;
        let result = parse_deadline_response(json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].description, "Submit report");
        assert_eq!(result[0].deadline_date, "2026-03-20");
        assert_eq!(result[0].deadline_source, "by March 20");
        assert!(result[0].is_explicit);
    }

    #[test]
    fn test_parse_deadline_response_empty() {
        let json = r#"{"deadlines": []}"#;
        let result = parse_deadline_response(json);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_deadline_response_malformed() {
        let result = parse_deadline_response("not json at all");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_deadline_response_partial_json() {
        let result = parse_deadline_response(r#"{"deadlines": [{"description": "test"}"#);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_deadline_response_markdown_fenced() {
        let json = "```json\n{\"deadlines\": [{\"description\": \"Review PR\", \"deadline_date\": \"2026-03-18\", \"deadline_source\": \"by Monday\", \"is_explicit\": true}]}\n```";
        let result = parse_deadline_response(json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].description, "Review PR");
    }

    #[test]
    fn test_parse_deadline_response_multiple_deadlines() {
        let json = r#"{"deadlines": [
            {"description": "Submit report", "deadline_date": "2026-03-15", "deadline_source": "by March 15", "is_explicit": true},
            {"description": "Respond ASAP", "deadline_date": "2026-03-14", "deadline_source": "ASAP", "is_explicit": false}
        ]}"#;
        let result = parse_deadline_response(json);
        assert_eq!(result.len(), 2);
        assert!(result[0].is_explicit);
        assert!(!result[1].is_explicit);
    }

    #[test]
    fn test_deadline_crud_operations() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create a test account and message first
        let account_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', 'test@example.com')",
            rusqlite::params![account_id],
        ).unwrap();

        let msg_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, subject, body_text, is_read, is_starred, has_attachments)
             VALUES (?1, ?2, 'INBOX', 'Test', 'Body', 0, 0, 0)",
            rusqlite::params![msg_id, account_id],
        ).unwrap();

        // Insert a deadline
        let deadline_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO deadlines (id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, 'thread-1', 'Submit report', '2026-03-20', 'by March 20', 1)",
            rusqlite::params![deadline_id, msg_id],
        ).unwrap();

        // Read it back
        let mut stmt = conn.prepare(
            "SELECT id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit, completed, created_at FROM deadlines WHERE id = ?1"
        ).unwrap();
        let deadline: Deadline = stmt.query_row(rusqlite::params![deadline_id], deadline_from_row).unwrap();
        assert_eq!(deadline.description, "Submit report");
        assert_eq!(deadline.deadline_date, "2026-03-20");
        assert!(!deadline.completed);

        // Complete it
        conn.execute("UPDATE deadlines SET completed = 1 WHERE id = ?1", rusqlite::params![deadline_id]).unwrap();
        let completed: i32 = conn.query_row(
            "SELECT completed FROM deadlines WHERE id = ?1",
            rusqlite::params![deadline_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(completed, 1);

        // Delete it
        conn.execute("DELETE FROM deadlines WHERE id = ?1", rusqlite::params![deadline_id]).unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM deadlines WHERE id = ?1",
            rusqlite::params![deadline_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_deadline_unique_constraint() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', 'uniq@example.com')",
            rusqlite::params![account_id],
        ).unwrap();

        let msg_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, is_read, is_starred, has_attachments)
             VALUES (?1, ?2, 'INBOX', 0, 0, 0)",
            rusqlite::params![msg_id, account_id],
        ).unwrap();

        // First insert succeeds
        let id1 = uuid::Uuid::new_v4().to_string();
        let rows = conn.execute(
            "INSERT OR IGNORE INTO deadlines (id, message_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, 'Same task', '2026-03-20', 'by March 20', 1)",
            rusqlite::params![id1, msg_id],
        ).unwrap();
        assert_eq!(rows, 1);

        // Duplicate (same message_id + description) is ignored
        let id2 = uuid::Uuid::new_v4().to_string();
        let rows = conn.execute(
            "INSERT OR IGNORE INTO deadlines (id, message_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, 'Same task', '2026-03-20', 'by March 20', 1)",
            rusqlite::params![id2, msg_id],
        ).unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn test_deadline_date_ordering() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', 'order@example.com')",
            rusqlite::params![account_id],
        ).unwrap();

        let msg_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, is_read, is_starred, has_attachments)
             VALUES (?1, ?2, 'INBOX', 0, 0, 0)",
            rusqlite::params![msg_id, account_id],
        ).unwrap();

        // Insert deadlines with different dates
        for (i, date) in ["2026-03-25", "2026-03-15", "2026-03-20"].iter().enumerate() {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO deadlines (id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit)
                 VALUES (?1, ?2, 'thread-ord', ?3, ?4, 'source', 1)",
                rusqlite::params![id, msg_id, format!("Task {}", i), date],
            ).unwrap();
        }

        // Query ordered by date
        let mut stmt = conn.prepare(
            "SELECT id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit, completed, created_at
             FROM deadlines WHERE thread_id = 'thread-ord' ORDER BY deadline_date ASC"
        ).unwrap();
        let deadlines: Vec<Deadline> = stmt.query_map([], deadline_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(deadlines.len(), 3);
        assert_eq!(deadlines[0].deadline_date, "2026-03-15");
        assert_eq!(deadlines[1].deadline_date, "2026-03-20");
        assert_eq!(deadlines[2].deadline_date, "2026-03-25");
    }

    #[test]
    fn test_deadline_upcoming_window_filtering() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', 'window@example.com')",
            rusqlite::params![account_id],
        ).unwrap();

        let msg_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, is_read, is_starred, has_attachments)
             VALUES (?1, ?2, 'INBOX', 0, 0, 0)",
            rusqlite::params![msg_id, account_id],
        ).unwrap();

        // Insert deadlines: one within 7 days window, one far in the future
        let today = chrono::Local::now();
        let near_date = (today + chrono::Duration::days(3)).format("%Y-%m-%d").to_string();
        let far_date = (today + chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
        let end_date = (today + chrono::Duration::days(7)).format("%Y-%m-%d").to_string();

        let id1 = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO deadlines (id, message_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, 'Near task', ?3, 'soon', 1)",
            rusqlite::params![id1, msg_id, near_date],
        ).unwrap();

        let id2 = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO deadlines (id, message_id, description, deadline_date, deadline_source, is_explicit)
             VALUES (?1, ?2, 'Far task', ?3, 'later', 1)",
            rusqlite::params![id2, msg_id, far_date],
        ).unwrap();

        // Filter by upcoming 7-day window
        let mut stmt = conn.prepare(
            "SELECT id, message_id, thread_id, description, deadline_date, deadline_source, is_explicit, completed, created_at
             FROM deadlines WHERE completed = 0 AND deadline_date <= ?1 ORDER BY deadline_date ASC"
        ).unwrap();
        let deadlines: Vec<Deadline> = stmt.query_map(rusqlite::params![end_date], deadline_from_row)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(deadlines.len(), 1);
        assert_eq!(deadlines[0].description, "Near task");
    }

    #[test]
    fn test_extract_json_from_markdown() {
        let raw = "```json\n{\"deadlines\":[]}\n```";
        let extracted = extract_json(raw);
        assert_eq!(extracted, "{\"deadlines\":[]}");
    }

    #[test]
    fn test_extract_json_plain() {
        let raw = "{\"deadlines\":[]}";
        let extracted = extract_json(raw);
        assert_eq!(extracted, "{\"deadlines\":[]}");
    }

    #[test]
    fn test_extract_json_with_text_prefix() {
        let raw = "Here is the result: {\"deadlines\":[]}";
        let extracted = extract_json(raw);
        assert_eq!(extracted, "{\"deadlines\":[]}");
    }
}
