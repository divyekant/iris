use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub event_name: String,
    pub approximate_date: String,
    pub date_precision: String,
    pub source_message_id: Option<String>,
    pub account_id: Option<String>,
    pub confidence: f64,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct TemporalSearchRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct ResolvedDateRange {
    pub description: String,
    pub start_date: String,
    pub end_date: String,
    pub matched_event: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Serialize)]
pub struct TemporalMessage {
    pub id: String,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub date: Option<i64>,
    pub snippet: Option<String>,
    pub is_read: bool,
}

#[derive(Debug, Serialize)]
pub struct TemporalSearchResponse {
    pub resolved_range: Option<ResolvedDateRange>,
    pub messages: Vec<TemporalMessage>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct TimelineParams {
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub events: Vec<TimelineEvent>,
    pub total: i64,
}

// --- AI resolution types ---

#[derive(Debug, Deserialize)]
struct AiTemporalResolution {
    description: String,
    start_date: String,
    end_date: String,
    confidence: Option<f64>,
}

// --- AI prompt ---

fn build_temporal_prompt(query: &str, events_context: &str, today: &str) -> String {
    format!(
        r#"You are a temporal reasoning engine for email search. Given a natural-language query referencing time, resolve it to a concrete date range.

Known events from this user's email timeline:
{events_context}

Today's date: {today}

Query: "{query}"

Instructions:
1. Identify the temporal reference in the query
2. Match it against known events, or infer a date from context
3. Return a JSON object with:
   - "description": human-readable explanation (e.g., "around the v2 launch in Oct 2025")
   - "start_date": ISO date string (YYYY-MM-DD) for the range start
   - "end_date": ISO date string (YYYY-MM-DD) for the range end
   - "confidence": 0.0 to 1.0

Use a +-2 week window for approximate events, +-1 week for precise dates.
If the query has no temporal reference, use the last 30 days.

Respond ONLY with the JSON object."#
    )
}

const TEMPORAL_SYSTEM_PROMPT: &str =
    "You resolve natural-language temporal references to concrete date ranges. Return ONLY valid JSON.";

async fn resolve_temporal_reference(
    providers: &crate::ai::provider::ProviderPool,
    query: &str,
    events_context: &str,
) -> Option<AiTemporalResolution> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let prompt = build_temporal_prompt(query, events_context, &today);
    let response = providers
        .generate(&prompt, Some(TEMPORAL_SYSTEM_PROMPT))
        .await?;

    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    serde_json::from_str::<AiTemporalResolution>(json_str).ok()
}

// --- Handlers ---

/// POST /api/search/temporal -- temporal search
pub async fn temporal_search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TemporalSearchRequest>,
) -> Result<Json<TemporalSearchResponse>, StatusCode> {
    if req.query.trim().is_empty() {
        return Ok(Json(TemporalSearchResponse {
            resolved_range: None,
            messages: Vec::new(),
            total: 0,
        }));
    }

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Gather known events for context
    let events_context = {
        let mut stmt = conn
            .prepare(
                "SELECT event_name, approximate_date, date_precision FROM timeline_events ORDER BY approximate_date DESC LIMIT 50",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let rows: Vec<String> = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let date: String = row.get(1)?;
                let precision: String = row.get(2)?;
                Ok(format!("- {} (approx. {}, precision: {})", name, date, precision))
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            "(No known events yet)".to_string()
        } else {
            rows.join("\n")
        }
    };

    // Resolve the temporal reference via AI
    let resolution = resolve_temporal_reference(&state.providers, &req.query, &events_context).await;

    let resolved_range = match resolution {
        Some(ref r) => Some(ResolvedDateRange {
            description: r.description.clone(),
            start_date: r.start_date.clone(),
            end_date: r.end_date.clone(),
            matched_event: None, // Could be enriched in future
            confidence: r.confidence.unwrap_or(0.7),
        }),
        None => {
            // Fallback: last 30 days
            let end = chrono::Local::now().format("%Y-%m-%d").to_string();
            let start = (chrono::Local::now() - chrono::Duration::days(30))
                .format("%Y-%m-%d")
                .to_string();
            Some(ResolvedDateRange {
                description: "Last 30 days (could not resolve temporal reference)".to_string(),
                start_date: start,
                end_date: end,
                matched_event: None,
                confidence: 0.3,
            })
        }
    };

    // Parse dates to timestamps for querying
    let (start_ts, end_ts) = if let Some(ref range) = resolved_range {
        let start = chrono::NaiveDate::parse_from_str(&range.start_date, "%Y-%m-%d")
            .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
            .unwrap_or(0);
        let end = chrono::NaiveDate::parse_from_str(&range.end_date, "%Y-%m-%d")
            .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc().timestamp())
            .unwrap_or(i64::MAX);
        (start, end)
    } else {
        (0, i64::MAX)
    };

    // Search messages in the resolved date range
    // Also do a keyword search on the non-temporal part of the query
    let mut stmt = conn
        .prepare(
            "SELECT id, thread_id, subject, from_address, from_name, date, snippet, is_read
             FROM messages
             WHERE date BETWEEN ?1 AND ?2
             AND folder NOT IN ('Trash', 'Spam')
             ORDER BY date DESC
             LIMIT 50",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let messages: Vec<TemporalMessage> = stmt
        .query_map(rusqlite::params![start_ts, end_ts], |row| {
            Ok(TemporalMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                subject: row.get(2)?,
                from_address: row.get(3)?,
                from_name: row.get(4)?,
                date: row.get(5)?,
                snippet: row.get(6)?,
                is_read: row.get(7)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let total = messages.len();

    Ok(Json(TemporalSearchResponse {
        resolved_range,
        messages,
        total,
    }))
}

/// GET /api/timeline?limit=20 -- list recent timeline events
pub async fn list_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<TimelineParams>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(20).min(100);

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM timeline_events", [], |row| row.get(0))
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT id, event_name, approximate_date, date_precision, source_message_id, account_id, confidence, created_at
             FROM timeline_events
             ORDER BY approximate_date DESC
             LIMIT ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let events: Vec<TimelineEvent> = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok(TimelineEvent {
                id: row.get(0)?,
                event_name: row.get(1)?,
                approximate_date: row.get(2)?,
                date_precision: row.get(3)?,
                source_message_id: row.get(4)?,
                account_id: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(TimelineResponse { events, total }))
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;
    use uuid::Uuid;

    fn setup_test_data() -> (crate::db::DbPool, String, String) {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = Account::create(
            &conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "test@example.com".to_string(),
                display_name: Some("Test User".to_string()),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("test@example.com".to_string()),
                password: Some("secret".to_string()),
            },
        );

        let msg = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<test@example.com>".to_string()),
            thread_id: Some("thread-1".to_string()),
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: None,
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Project Update".to_string()),
            date: Some(1700000000),
            snippet: Some("Here's the latest update".to_string()),
            body_text: Some("Project update body".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: None,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };

        let msg_id = InsertMessage::insert(&conn, &msg).expect("insert");
        (pool, account.id, msg_id)
    }

    #[test]
    fn test_timeline_events_table_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='timeline_events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_timeline_event() {
        let (pool, account_id, msg_id) = setup_test_data();
        let conn = pool.get().unwrap();

        let event_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision, source_message_id, account_id)
             VALUES (?1, 'V2 Launch', '2025-10-15', 'week', ?2, ?3)",
            rusqlite::params![event_id, msg_id, account_id],
        )
        .unwrap();

        let name: String = conn
            .query_row(
                "SELECT event_name FROM timeline_events WHERE id = ?1",
                rusqlite::params![event_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "V2 Launch");
    }

    #[test]
    fn test_timeline_events_ordering() {
        let (pool, account_id, _msg_id) = setup_test_data();
        let conn = pool.get().unwrap();

        // Insert events with different dates
        for (name, date) in &[
            ("Event A", "2025-01-01"),
            ("Event C", "2025-03-01"),
            ("Event B", "2025-02-01"),
        ] {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO timeline_events (id, event_name, approximate_date, account_id) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, name, date, account_id],
            )
            .unwrap();
        }

        let mut stmt = conn
            .prepare("SELECT event_name FROM timeline_events ORDER BY approximate_date DESC")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(names, vec!["Event C", "Event B", "Event A"]);
    }

    #[test]
    fn test_timeline_event_precision_constraint() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Valid precision
        let id = Uuid::new_v4().to_string();
        let result = conn.execute(
            "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision) VALUES (?1, 'Test', '2025-01-01', 'month')",
            rusqlite::params![id],
        );
        assert!(result.is_ok());

        // Invalid precision
        let id2 = Uuid::new_v4().to_string();
        let result2 = conn.execute(
            "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision) VALUES (?1, 'Test2', '2025-01-01', 'invalid')",
            rusqlite::params![id2],
        );
        assert!(result2.is_err());
    }

    #[test]
    fn test_temporal_prompt_contains_context() {
        let prompt = build_temporal_prompt(
            "emails from around when we launched v2",
            "- V2 Launch (approx. 2025-10-15, precision: week)",
            "2026-03-16",
        );
        assert!(prompt.contains("launched v2"));
        assert!(prompt.contains("V2 Launch"));
        assert!(prompt.contains("2026-03-16"));
    }

    #[test]
    fn test_ai_temporal_resolution_parsing() {
        let json = r#"{"description": "around the v2 launch", "start_date": "2025-10-01", "end_date": "2025-10-29", "confidence": 0.85}"#;
        let result: AiTemporalResolution = serde_json::from_str(json).unwrap();
        assert_eq!(result.description, "around the v2 launch");
        assert_eq!(result.start_date, "2025-10-01");
        assert_eq!(result.end_date, "2025-10-29");
        assert!((result.confidence.unwrap() - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_message_date_range_query() {
        let (pool, account_id, _msg_id) = setup_test_data();
        let conn = pool.get().unwrap();

        // Insert messages with different dates
        for (i, date) in [1700000000i64, 1700100000, 1700200000].iter().enumerate() {
            let msg = InsertMessage {
                account_id: account_id.clone(),
                message_id: Some(format!("<msg-{}@example.com>", i)),
                thread_id: None,
                folder: "INBOX".to_string(),
                from_address: Some("a@b.com".to_string()),
                from_name: None,
                to_addresses: None,
                cc_addresses: None,
                bcc_addresses: None,
                subject: Some(format!("Msg {}", i)),
                date: Some(*date),
                snippet: None,
                body_text: None,
                body_html: None,
                is_read: false,
                is_starred: false,
                is_draft: false,
                labels: None,
                uid: Some(100 + i as i64),
                modseq: None,
                raw_headers: None,
                has_attachments: false,
                attachment_names: None,
                size_bytes: None,
                list_unsubscribe: None,
                list_unsubscribe_post: false,
            };
            let _ = InsertMessage::insert(&conn, &msg);
        }

        // Query range covering first two messages
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE date BETWEEN ?1 AND ?2",
                rusqlite::params![1700000000i64, 1700150000i64],
                |row| row.get(0),
            )
            .unwrap();
        // Original + 2 new ones in range
        assert!(count >= 2);
    }

    #[test]
    fn test_entity_extract_job_type_allowed() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // entity_extract should be a valid job type after migration 053
        let result = conn.execute(
            "INSERT INTO processing_jobs (job_type, message_id) VALUES ('entity_extract', 'test-msg')",
            [],
        );
        assert!(result.is_ok());
    }
}
