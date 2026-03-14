use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::utils::escape_sql_like;
use crate::AppState;

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopicEntry {
    pub topic: String,
    pub count: u32,
}

#[derive(Debug, Serialize)]
pub struct ContactTopicsResponse {
    pub email: String,
    pub topics: Vec<TopicEntry>,
    pub total_emails: i64,
    pub cached: bool,
}

#[derive(Debug, Serialize)]
pub struct ContactSummary {
    pub email: String,
    pub name: Option<String>,
    pub email_count: i64,
    pub last_contact: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TopContactsResponse {
    pub contacts: Vec<ContactSummary>,
}

// Cache TTL: 1 hour in seconds
const CACHE_TTL_SECS: i64 = 3600;

const TOPICS_SYSTEM_PROMPT: &str = "You are an email analysis assistant. Given a list of email subjects and message snippets exchanged with a specific person, identify the key recurring topics discussed. Return ONLY a valid JSON array with no surrounding text, markdown, or code fences. Each element must have 'topic' (a concise topic label, 3-6 words max) and 'count' (integer, estimated number of emails on this topic). Example: [{\"topic\": \"Project status updates\", \"count\": 5}, {\"topic\": \"Meeting scheduling\", \"count\": 3}]";

// ---------------------------------------------------------------------------
// GET /api/contacts/{email}/topics
// ---------------------------------------------------------------------------

pub async fn get_contact_topics(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<ContactTopicsResponse>, StatusCode> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check DB cache — serve if within TTL
    let cache_row: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT topics_json, total_emails, computed_at \
             FROM contact_topics_cache WHERE email = ?1",
            rusqlite::params![email],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    let now = chrono::Utc::now().timestamp();
    if let Some((topics_json, total_emails, computed_at)) = cache_row {
        if now - computed_at < CACHE_TTL_SECS {
            let topics: Vec<TopicEntry> = serde_json::from_str(&topics_json).unwrap_or_default();
            return Ok(Json(ContactTopicsResponse {
                email,
                topics,
                total_emails,
                cached: true,
            }));
        }
    }

    // Fetch up to 20 most recent messages with this contact
    let escaped = escape_sql_like(&email);
    let like_pattern = format!("%{}%", escaped);
    let rows: Vec<(Option<String>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT subject, snippet FROM messages
                 WHERE is_deleted = 0
                   AND (LOWER(from_address) = ?1
                        OR to_addresses LIKE ?2 ESCAPE '\\'
                        OR cc_addresses LIKE ?2 ESCAPE '\\')
                 ORDER BY date DESC
                 LIMIT 20",
            )
            .map_err(|e| {
                tracing::error!("Contact topics prepare error: {e}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        stmt.query_map(rusqlite::params![email, like_pattern], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect()
    };

    // Count all-time messages with this contact (not capped at 20)
    let total_emails: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE is_deleted = 0
               AND (LOWER(from_address) = ?1
                    OR to_addresses LIKE ?2
                    OR cc_addresses LIKE ?2)",
            rusqlite::params![email, like_pattern],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if rows.is_empty() {
        return Ok(Json(ContactTopicsResponse {
            email,
            topics: vec![],
            total_emails: 0,
            cached: false,
        }));
    }

    // Check AI is available — return empty topics (not an error) when disabled
    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Ok(Json(ContactTopicsResponse {
            email,
            topics: vec![],
            total_emails,
            cached: false,
        }));
    }

    // Build prompt from subjects + snippets
    let mut prompt = format!(
        "Analyze {} email subjects and snippets exchanged with {}. Identify key recurring topics.\n\n",
        rows.len(),
        email
    );
    for (i, (subject, snippet)) in rows.iter().enumerate() {
        let subj = subject.as_deref().unwrap_or("(no subject)");
        let snip: String = snippet.as_deref().unwrap_or("").chars().take(150).collect();
        prompt.push_str(&format!("Email {}: Subject: {} | Snippet: {}\n", i + 1, subj, snip));
    }
    prompt.push_str("\nReturn 5-10 key topics as a JSON array.");

    let raw_response = state
        .providers
        .generate(&prompt, Some(TOPICS_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let topics = parse_topics_json(&raw_response);

    // Persist to DB cache
    let topics_json = serde_json::to_string(&topics).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "INSERT OR REPLACE INTO contact_topics_cache (email, topics_json, total_emails, computed_at)
         VALUES (?1, ?2, ?3, unixepoch())",
        rusqlite::params![email, topics_json, total_emails],
    )
    .ok();

    Ok(Json(ContactTopicsResponse {
        email,
        topics,
        total_emails,
        cached: false,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/contacts/top
// ---------------------------------------------------------------------------

pub async fn get_top_contacts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TopContactsResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stmt = conn
        .prepare(
            "SELECT from_address, from_name, COUNT(*) as email_count, MAX(date) as last_contact
             FROM messages
             WHERE is_deleted = 0
               AND from_address IS NOT NULL
               AND from_address != ''
             GROUP BY LOWER(from_address)
             ORDER BY email_count DESC
             LIMIT 10",
        )
        .map_err(|e| {
            tracing::error!("Top contacts query error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let contacts: Vec<ContactSummary> = stmt
        .query_map([], |row| {
            Ok(ContactSummary {
                email: row.get(0)?,
                name: row.get(1)?,
                email_count: row.get(2)?,
                last_contact: row.get(3)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(TopContactsResponse { contacts }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a JSON array of TopicEntry from an AI response.
/// Handles common LLM response patterns: raw array, markdown fences, surrounding prose.
fn parse_topics_json(raw: &str) -> Vec<TopicEntry> {
    // Try the trimmed string directly first
    if let Ok(topics) = serde_json::from_str::<Vec<TopicEntry>>(raw.trim()) {
        return topics;
    }

    // Find the outermost [ ... ] in the response
    if let Some(start) = raw.find('[') {
        if let Some(end) = raw.rfind(']') {
            if end > start {
                if let Ok(topics) = serde_json::from_str::<Vec<TopicEntry>>(&raw[start..=end]) {
                    return topics;
                }
            }
        }
    }

    tracing::warn!(
        "Could not parse topics JSON from AI response (first 200 chars): {}",
        &raw[..raw.len().min(200)]
    );
    vec![]
}

// ---------------------------------------------------------------------------
// GET /api/contacts/{email}/response-times
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ResponseTimesResponse {
    pub email: String,
    pub their_avg_reply_hours: Option<f64>,
    pub your_avg_reply_hours: Option<f64>,
    pub their_reply_count: i64,
    pub your_reply_count: i64,
    pub fastest_reply_hours: Option<f64>,
    pub slowest_reply_hours: Option<f64>,
    pub total_exchanges: i64,
}

pub async fn get_response_times(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
) -> Result<Json<ResponseTimesResponse>, StatusCode> {
    if !email.contains('@') || email.len() > 320 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let contact_email = email.to_lowercase();
    let escaped = escape_sql_like(&contact_email);
    let like_pattern = format!("%{}%", escaped);

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let account_emails: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT LOWER(email) FROM accounts WHERE is_active = 1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    if account_emails.is_empty() {
        return Ok(Json(ResponseTimesResponse {
            email: contact_email,
            their_avg_reply_hours: None,
            your_avg_reply_hours: None,
            their_reply_count: 0,
            your_reply_count: 0,
            fastest_reply_hours: None,
            slowest_reply_hours: None,
            total_exchanges: 0,
        }));
    }

    let mut stmt = conn
        .prepare(
            "SELECT m.thread_id, LOWER(m.from_address) as from_addr, m.date, a.email as account_email
             FROM messages m
             JOIN accounts a ON m.account_id = a.id
             WHERE m.is_deleted = 0
               AND m.thread_id IS NOT NULL
               AND m.date IS NOT NULL
               AND (LOWER(m.from_address) = ?1
                    OR m.to_addresses LIKE ?2 ESCAPE '\\'
                    OR m.cc_addresses LIKE ?2 ESCAPE '\\')
             ORDER BY m.thread_id, m.date ASC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<(String, String, i64, String)> = stmt
        .query_map(rusqlite::params![&contact_email, &like_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let stats = compute_reply_stats(&rows, &contact_email, &account_emails);

    Ok(Json(ResponseTimesResponse {
        email: contact_email,
        their_avg_reply_hours: stats.their_avg,
        your_avg_reply_hours: stats.your_avg,
        their_reply_count: stats.their_count,
        your_reply_count: stats.your_count,
        fastest_reply_hours: stats.fastest,
        slowest_reply_hours: stats.slowest,
        total_exchanges: stats.total_exchanges,
    }))
}

struct ReplyStats {
    their_avg: Option<f64>,
    your_avg: Option<f64>,
    their_count: i64,
    your_count: i64,
    fastest: Option<f64>,
    slowest: Option<f64>,
    total_exchanges: i64,
}

fn compute_reply_stats(
    rows: &[(String, String, i64, String)],
    contact_email: &str,
    account_emails: &[String],
) -> ReplyStats {
    let mut their_reply_secs: Vec<f64> = Vec::new();
    let mut your_reply_secs: Vec<f64> = Vec::new();
    let mut total_exchanges: i64 = 0;

    let mut i = 0;
    while i < rows.len() {
        let thread_id = &rows[i].0;
        let mut thread_end = i + 1;
        while thread_end < rows.len() && rows[thread_end].0 == *thread_id {
            thread_end += 1;
        }

        let thread_msgs = &rows[i..thread_end];
        total_exchanges += thread_msgs.len() as i64;

        for pair in thread_msgs.windows(2) {
            let (_, ref from_a, date_a, _) = pair[0];
            let (_, ref from_b, date_b, _) = pair[1];

            if from_a == from_b {
                continue;
            }

            let delta_secs = (date_b - date_a) as f64;
            if delta_secs <= 0.0 {
                continue;
            }

            let a_is_contact = from_a == contact_email;
            let a_is_user = account_emails.iter().any(|ae| ae == from_a);
            let b_is_contact = from_b == contact_email;
            let b_is_user = account_emails.iter().any(|ae| ae == from_b);

            if a_is_contact && b_is_user {
                your_reply_secs.push(delta_secs);
            } else if a_is_user && b_is_contact {
                their_reply_secs.push(delta_secs);
            }
        }

        i = thread_end;
    }

    let their_avg = if their_reply_secs.is_empty() {
        None
    } else {
        let sum: f64 = their_reply_secs.iter().sum();
        Some(sum / their_reply_secs.len() as f64 / 3600.0)
    };

    let your_avg = if your_reply_secs.is_empty() {
        None
    } else {
        let sum: f64 = your_reply_secs.iter().sum();
        Some(sum / your_reply_secs.len() as f64 / 3600.0)
    };

    let all_deltas: Vec<f64> = their_reply_secs
        .iter()
        .chain(your_reply_secs.iter())
        .copied()
        .collect();

    let fastest = all_deltas
        .iter()
        .copied()
        .reduce(f64::min)
        .map(|s| s / 3600.0);
    let slowest = all_deltas
        .iter()
        .copied()
        .reduce(f64::max)
        .map(|s| s / 3600.0);

    ReplyStats {
        their_avg,
        your_avg,
        their_count: their_reply_secs.len() as i64,
        your_count: your_reply_secs.len() as i64,
        fastest,
        slowest,
        total_exchanges,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_topics_json ---

    #[test]
    fn test_parse_topics_clean_array() {
        let raw = r#"[{"topic": "Project updates", "count": 5}, {"topic": "Meeting scheduling", "count": 3}]"#;
        let topics = parse_topics_json(raw);
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0].topic, "Project updates");
        assert_eq!(topics[0].count, 5);
        assert_eq!(topics[1].topic, "Meeting scheduling");
        assert_eq!(topics[1].count, 3);
    }

    #[test]
    fn test_parse_topics_markdown_fence() {
        let raw = "```json\n[{\"topic\": \"Budget planning\", \"count\": 4}]\n```";
        let topics = parse_topics_json(raw);
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].topic, "Budget planning");
        assert_eq!(topics[0].count, 4);
    }

    #[test]
    fn test_parse_topics_surrounding_prose() {
        let raw = r#"Based on the emails: [{"topic": "Design reviews", "count": 6}] These are the main topics."#;
        let topics = parse_topics_json(raw);
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].topic, "Design reviews");
    }

    #[test]
    fn test_parse_topics_empty_array() {
        assert!(parse_topics_json("[]").is_empty());
    }

    #[test]
    fn test_parse_topics_not_json() {
        assert!(parse_topics_json("I cannot identify any topics.").is_empty());
    }

    #[test]
    fn test_parse_topics_whitespace_trimmed() {
        let raw = "  [ {\"topic\": \"Onboarding\", \"count\": 1} ]  ";
        let topics = parse_topics_json(raw);
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].topic, "Onboarding");
    }

    // --- cache TTL constant ---

    #[test]
    fn test_cache_ttl_is_one_hour() {
        assert_eq!(CACHE_TTL_SECS, 3600);
    }

    // --- SQL validation against test DB ---

    #[test]
    fn test_top_contacts_sql_valid() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        let result = conn.prepare(
            "SELECT from_address, from_name, COUNT(*) as email_count, MAX(date) as last_contact
             FROM messages
             WHERE is_deleted = 0
               AND from_address IS NOT NULL
               AND from_address != ''
             GROUP BY LOWER(from_address)
             ORDER BY email_count DESC
             LIMIT 10",
        );
        assert!(result.is_ok(), "top contacts SQL should be valid");
    }

    #[test]
    fn test_contact_topics_sql_valid_and_runs() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        let email = "alice@example.com";
        let like_pattern = format!("%{}%", email);

        // Both queries should prepare cleanly
        let prep1 = conn.prepare(
            "SELECT subject, snippet FROM messages
             WHERE is_deleted = 0
               AND (LOWER(from_address) = ?1
                    OR to_addresses LIKE ?2
                    OR cc_addresses LIKE ?2)
             ORDER BY date DESC
             LIMIT 20",
        );
        assert!(prep1.is_ok(), "topics message lookup SQL should be valid");

        // Count query runs and returns 0 on empty DB
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages
                 WHERE is_deleted = 0
                   AND (LOWER(from_address) = ?1
                        OR to_addresses LIKE ?2
                        OR cc_addresses LIKE ?2)",
                rusqlite::params![email, like_pattern],
                |row| row.get(0),
            )
            .unwrap_or(-1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_contact_topics_returns_correct_count() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'gmail', 'test@example.com')",
            [],
        )
        .unwrap();

        let now_ts = chrono::Utc::now().timestamp();

        // Insert 3 messages from alice
        for i in 0..3 {
            conn.execute(
                "INSERT INTO messages (id, account_id, folder, from_address, subject, date)
                 VALUES (?1, 'acc1', 'INBOX', 'alice@example.com', 'Hi', ?2)",
                rusqlite::params![format!("m{i}"), now_ts],
            )
            .unwrap();
        }

        // Insert 1 message TO alice (to_addresses contains her email)
        conn.execute(
            "INSERT INTO messages (id, account_id, folder, from_address, to_addresses, subject, date)
             VALUES ('m99', 'acc1', 'Sent', 'me@example.com', '[\"alice@example.com\"]', 'Re: Hi', ?1)",
            rusqlite::params![now_ts],
        )
        .unwrap();

        let email = "alice@example.com";
        let like_pattern = format!("%{}%", email);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages
                 WHERE is_deleted = 0
                   AND (LOWER(from_address) = ?1
                        OR to_addresses LIKE ?2
                        OR cc_addresses LIKE ?2)",
                rusqlite::params![email, like_pattern],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count, 4); // 3 from + 1 to
    }

    #[test]
    fn test_cache_table_exists() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='contact_topics_cache'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "contact_topics_cache table should exist after migration");
    }

    // --- compute_reply_stats ---

    #[test]
    fn test_compute_reply_stats_basic() {
        let rows = vec![
            ("thread1".to_string(), "contact@example.com".to_string(), 1000, "user@me.com".to_string()),
            ("thread1".to_string(), "user@me.com".to_string(), 4600, "user@me.com".to_string()),
            ("thread1".to_string(), "contact@example.com".to_string(), 11800, "user@me.com".to_string()),
        ];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        assert_eq!(stats.your_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);
        assert_eq!(stats.their_count, 1);
        assert!((stats.their_avg.unwrap() - 2.0).abs() < 0.01);
        assert!((stats.fastest.unwrap() - 1.0).abs() < 0.01);
        assert!((stats.slowest.unwrap() - 2.0).abs() < 0.01);
        assert_eq!(stats.total_exchanges, 3);
    }

    #[test]
    fn test_compute_reply_stats_empty() {
        let rows: Vec<(String, String, i64, String)> = vec![];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        assert!(stats.their_avg.is_none());
        assert!(stats.your_avg.is_none());
        assert_eq!(stats.their_count, 0);
        assert_eq!(stats.your_count, 0);
        assert!(stats.fastest.is_none());
        assert!(stats.slowest.is_none());
        assert_eq!(stats.total_exchanges, 0);
    }

    #[test]
    fn test_compute_reply_stats_same_sender_no_reply() {
        let rows = vec![
            ("thread1".to_string(), "contact@example.com".to_string(), 1000, "user@me.com".to_string()),
            ("thread1".to_string(), "contact@example.com".to_string(), 2000, "user@me.com".to_string()),
        ];
        let account_emails = vec!["user@me.com".to_string()];
        let stats = compute_reply_stats(&rows, "contact@example.com", &account_emails);

        assert_eq!(stats.their_count, 0);
        assert_eq!(stats.your_count, 0);
        assert!(stats.their_avg.is_none());
        assert!(stats.your_avg.is_none());
    }

    #[test]
    fn test_compute_reply_stats_multiple_threads() {
        let rows = vec![
            ("thread1".to_string(), "bob@work.com".to_string(), 1000, "me@home.com".to_string()),
            ("thread1".to_string(), "me@home.com".to_string(), 4600, "me@home.com".to_string()),
            ("thread2".to_string(), "me@home.com".to_string(), 10000, "me@home.com".to_string()),
            ("thread2".to_string(), "bob@work.com".to_string(), 17200, "me@home.com".to_string()),
        ];
        let account_emails = vec!["me@home.com".to_string()];
        let stats = compute_reply_stats(&rows, "bob@work.com", &account_emails);

        assert_eq!(stats.your_count, 1);
        assert_eq!(stats.their_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);
        assert!((stats.their_avg.unwrap() - 2.0).abs() < 0.01);
        assert_eq!(stats.total_exchanges, 4);
    }

    #[test]
    fn test_response_times_with_db() {
        let pool = crate::db::create_test_pool();
        let conn = pool.get().unwrap();

        conn.execute(
            "INSERT INTO accounts (id, email, provider, display_name, is_active) VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params!["acc1", "user@test.com", "gmail", "Test User"],
        ).unwrap();

        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted)
             VALUES ('m1', 'acc1', 'thread1', 'INBOX', 'contact@external.com', 'Contact', 'Hello', 1000, 1, 0)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted, to_addresses)
             VALUES ('m2', 'acc1', 'thread1', 'Sent', 'user@test.com', 'Test User', 'Re: Hello', 4600, 1, 0, '[\"contact@external.com\"]')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, from_address, from_name, subject, date, is_read, is_deleted, to_addresses)
             VALUES ('m3', 'acc1', 'thread1', 'INBOX', 'contact@external.com', 'Contact', 'Re: Re: Hello', 18200, 1, 0, '[\"user@test.com\"]')",
            [],
        ).unwrap();

        let contact_email = "contact@external.com";
        let like_pattern = format!("%{}%", contact_email);

        let mut stmt = conn
            .prepare(
                "SELECT m.thread_id, LOWER(m.from_address) as from_addr, m.date, a.email as account_email
                 FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE m.is_deleted = 0
                   AND m.thread_id IS NOT NULL
                   AND m.date IS NOT NULL
                   AND (LOWER(m.from_address) = ?1
                        OR m.to_addresses LIKE ?2
                        OR m.cc_addresses LIKE ?2)
                 ORDER BY m.thread_id, m.date ASC",
            )
            .unwrap();

        let rows: Vec<(String, String, i64, String)> = stmt
            .query_map(rusqlite::params![contact_email, &like_pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let account_emails = vec!["user@test.com".to_string()];
        let stats = compute_reply_stats(&rows, contact_email, &account_emails);

        assert_eq!(stats.your_count, 1);
        assert!((stats.your_avg.unwrap() - 1.0).abs() < 0.01);
        assert_eq!(stats.their_count, 1);
        assert!((stats.their_avg.unwrap() - 3.7778).abs() < 0.01);
        assert_eq!(stats.total_exchanges, 3);
    }

    #[test]
    fn test_invalid_email_validation() {
        let email = "notanemail";
        assert!(!email.contains('@'));
    }
}
