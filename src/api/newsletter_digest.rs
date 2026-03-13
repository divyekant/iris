use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DigestRequest {
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub senders: Option<Vec<String>>,
    pub max_emails: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DigestSource {
    pub sender: String,
    pub count: i64,
    pub subjects: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DigestResponse {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub sources: Vec<DigestSource>,
    pub generated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct SourceInfo {
    pub sender: String,
    pub sender_name: Option<String>,
    pub email_count: i64,
    pub last_received: Option<i64>,
    pub categories: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SourcesResponse {
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub senders: Option<Vec<String>>,
    pub max_emails: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PreviewMessage {
    pub id: String,
    pub from: Option<String>,
    pub subject: Option<String>,
    pub date: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub messages: Vec<PreviewMessage>,
    pub total_count: i64,
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DigestHistoryEntry {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub source_count: i64,
    pub message_count: i64,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub digests: Vec<DigestHistoryEntry>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the WHERE clause and params for newsletter queries.
/// Newsletters are identified by either:
/// 1. raw_headers containing 'List-Unsubscribe' (standard newsletter header)
/// 2. ai_category being 'Newsletters' or 'Promotions'
fn build_newsletter_query(
    date_from: Option<i64>,
    date_to: Option<i64>,
    senders: &Option<Vec<String>>,
    max_emails: Option<i64>,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let mut conditions = vec![
        "is_deleted = 0".to_string(),
        "is_draft = 0".to_string(),
        "(raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions'))".to_string(),
    ];
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(df) = date_from {
        conditions.push(format!("date >= ?{param_idx}"));
        params.push(Box::new(df));
        param_idx += 1;
    }

    if let Some(dt) = date_to {
        conditions.push(format!("date <= ?{param_idx}"));
        params.push(Box::new(dt));
        param_idx += 1;
    }

    if let Some(sender_list) = senders {
        if !sender_list.is_empty() {
            let placeholders: Vec<String> = sender_list
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", param_idx + i))
                .collect();
            conditions.push(format!("from_address IN ({})", placeholders.join(", ")));
            for s in sender_list {
                params.push(Box::new(s.clone()));
            }
            // param_idx is not used after this point, but keep for clarity
            let _ = param_idx + sender_list.len();
        }
    }

    let where_clause = conditions.join(" AND ");
    let limit = max_emails.unwrap_or(100).min(500);
    let sql = format!(
        "SELECT id, from_address, from_name, subject, snippet, date \
         FROM messages WHERE {} ORDER BY date DESC LIMIT {}",
        where_clause, limit
    );

    (sql, params)
}

// ---------------------------------------------------------------------------
// POST /api/ai/newsletter-digest — generate a digest
// ---------------------------------------------------------------------------

const DIGEST_SYSTEM_PROMPT: &str = "You are an email digest generator. Given a collection of newsletter emails grouped by sender, produce a concise digest summary. For each source, highlight the key takeaways, important announcements, or action items. Start with a short title line prefixed with 'TITLE: ' on its own line, then provide the digest summary. Be factual, concise, and organize by source. Use plain text with clear section headers.";

pub async fn generate_digest(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DigestRequest>,
) -> Result<Json<DigestResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

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

    struct EmailRow {
        id: String,
        from_address: Option<String>,
        from_name: Option<String>,
        subject: Option<String>,
        snippet: Option<String>,
        date: Option<i64>,
    }

    let rows: Vec<EmailRow> = {
        let (sql, params) = build_newsletter_query(req.date_from, req.date_to, &req.senders, req.max_emails);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            tracing::error!("Failed to prepare newsletter query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        stmt.query_map(param_refs.as_slice(), |row| {
            Ok(EmailRow {
                id: row.get(0)?,
                from_address: row.get(1)?,
                from_name: row.get(2)?,
                subject: row.get(3)?,
                snippet: row.get(4)?,
                date: row.get(5)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query newsletters: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    if rows.is_empty() {
        return Ok(Json(DigestResponse {
            id: uuid::Uuid::new_v4().to_string(),
            title: "No newsletters found".to_string(),
            summary: "No newsletter or promotional emails were found matching the specified criteria.".to_string(),
            sources: vec![],
            generated_at: chrono::Utc::now().timestamp(),
        }));
    }

    // Group by sender
    let mut by_sender: std::collections::BTreeMap<String, Vec<&EmailRow>> = std::collections::BTreeMap::new();
    for row in &rows {
        let sender = row.from_address.clone().unwrap_or_else(|| "unknown".to_string());
        by_sender.entry(sender).or_default().push(row);
    }

    // Build prompt
    let mut prompt = String::from("Here are the recent newsletter/promotional emails grouped by sender:\n\n");
    for (sender, emails) in &by_sender {
        let display_name = emails
            .first()
            .and_then(|e| e.from_name.as_deref())
            .unwrap_or(sender.as_str());
        prompt.push_str(&format!("--- {} ({}) ---\n", display_name, sender));
        for email in emails {
            let subj = email.subject.as_deref().unwrap_or("(no subject)");
            let snip = email.snippet.as_deref().unwrap_or("");
            let date_str = email
                .date
                .and_then(|d| chrono::DateTime::from_timestamp(d, 0))
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            prompt.push_str(&format!("  - [{}] {}: {}\n", date_str, subj, snip));
        }
        prompt.push('\n');
    }

    let ai_response = state
        .providers
        .generate(&prompt, Some(DIGEST_SYSTEM_PROMPT))
        .await
        .ok_or_else(|| {
            tracing::error!("AI digest generation failed: all providers returned None");
            StatusCode::BAD_GATEWAY
        })?;

    // Parse title from response (first line starting with "TITLE: ")
    let (title, summary) = if let Some(rest) = ai_response.strip_prefix("TITLE: ") {
        if let Some(newline_pos) = rest.find('\n') {
            let title = rest[..newline_pos].trim().to_string();
            let summary = rest[newline_pos..].trim().to_string();
            (title, summary)
        } else {
            (rest.trim().to_string(), String::new())
        }
    } else {
        ("Newsletter Digest".to_string(), ai_response.trim().to_string())
    };

    // Build sources list
    let sources: Vec<DigestSource> = by_sender
        .iter()
        .map(|(sender, emails)| DigestSource {
            sender: sender.clone(),
            count: emails.len() as i64,
            subjects: emails
                .iter()
                .filter_map(|e| e.subject.clone())
                .collect(),
        })
        .collect();

    let message_ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let message_ids_json = serde_json::to_string(&message_ids).unwrap_or_else(|_| "[]".to_string());
    let now = chrono::Utc::now().timestamp();
    let digest_id = uuid::Uuid::new_v4().to_string();

    // Store in DB
    conn.execute(
        "INSERT INTO newsletter_digests (id, title, summary, message_ids, source_count, message_count, date_from, date_to, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            digest_id,
            title,
            summary,
            message_ids_json,
            sources.len() as i64,
            rows.len() as i64,
            req.date_from,
            req.date_to,
            now,
        ],
    )
    .map_err(|e| {
        tracing::error!("Failed to store digest: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(DigestResponse {
        id: digest_id,
        title,
        summary,
        sources,
        generated_at: now,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/ai/newsletter-digest/sources — list detected newsletter senders
// ---------------------------------------------------------------------------

pub async fn list_sources(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SourcesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut stmt = conn
        .prepare(
            "SELECT from_address, from_name, COUNT(*) as cnt, MAX(date) as last_date,
                    GROUP_CONCAT(DISTINCT ai_category) as cats
             FROM messages
             WHERE is_deleted = 0 AND is_draft = 0
               AND (raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions'))
             GROUP BY from_address
             ORDER BY cnt DESC
             LIMIT 100",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare sources query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let sources: Vec<SourceInfo> = stmt
        .query_map([], |row| {
            let cats_str: Option<String> = row.get(4)?;
            let categories = cats_str
                .map(|s| s.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
                .unwrap_or_default();

            Ok(SourceInfo {
                sender: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                sender_name: row.get(1)?,
                email_count: row.get(2)?,
                last_received: row.get(3)?,
                categories,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query sources: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(SourcesResponse { sources }))
}

// ---------------------------------------------------------------------------
// POST /api/ai/newsletter-digest/preview — preview digest contents
// ---------------------------------------------------------------------------

pub async fn preview_digest(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let (sql, params) = build_newsletter_query(req.date_from, req.date_to, &req.senders, req.max_emails);
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        tracing::error!("Failed to prepare preview query: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let messages: Vec<PreviewMessage> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(PreviewMessage {
                id: row.get(0)?,
                from: row.get(1)?,
                subject: row.get(3)?,
                date: row.get(5)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query preview: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    let total_count = messages.len() as i64;

    Ok(Json(PreviewResponse {
        messages,
        total_count,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/ai/newsletter-digest/history — list past digests
// ---------------------------------------------------------------------------

pub async fn digest_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let limit = query.limit.unwrap_or(20).min(100);
    let offset = query.offset.unwrap_or(0).max(0);

    let mut stmt = conn
        .prepare(
            "SELECT id, title, summary, source_count, message_count, date_from, date_to, created_at
             FROM newsletter_digests
             ORDER BY created_at DESC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare history query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let digests: Vec<DigestHistoryEntry> = stmt
        .query_map(rusqlite::params![limit, offset], |row| {
            Ok(DigestHistoryEntry {
                id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                source_count: row.get(3)?,
                message_count: row.get(4)?,
                date_from: row.get(5)?,
                date_to: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query history: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(HistoryResponse { digests }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn setup_pool() -> crate::db::DbPool {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // Create the newsletter_digests table for tests
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS newsletter_digests (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                message_ids TEXT NOT NULL,
                source_count INTEGER NOT NULL DEFAULT 0,
                message_count INTEGER NOT NULL DEFAULT 0,
                date_from INTEGER,
                date_to INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )
        .unwrap();
        pool
    }

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "digest-test@example.com".to_string(),
            display_name: Some("Digest Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("digest-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_newsletter_msg(
        account_id: &str,
        from: &str,
        from_name: &str,
        subject: &str,
        date: i64,
        message_id_suffix: &str,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}@newsletter.example.com>", message_id_suffix)),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: Some(from_name.to_string()),
            to_addresses: Some(r#"["digest-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some(format!("Preview of {subject}")),
            body_text: Some(format!("Full body of {subject}")),
            body_html: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: None,
            modseq: None,
            raw_headers: Some("From: test\r\nList-Unsubscribe: <mailto:unsub@example.com>".to_string()),
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(2048),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn make_promo_msg(
        account_id: &str,
        from: &str,
        from_name: &str,
        subject: &str,
        date: i64,
        message_id_suffix: &str,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}@promo.example.com>", message_id_suffix)),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: Some(from_name.to_string()),
            to_addresses: Some(r#"["digest-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some(format!("Preview of {subject}")),
            body_text: Some(format!("Full body of {subject}")),
            body_html: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: None,
            modseq: None,
            raw_headers: None, // No List-Unsubscribe header — ai_category set after insert
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn make_regular_msg(
        account_id: &str,
        from: &str,
        subject: &str,
        date: i64,
        message_id_suffix: &str,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}@regular.example.com>", message_id_suffix)),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: Some("Regular Sender".to_string()),
            to_addresses: Some(r#"["digest-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(date),
            snippet: Some(format!("Preview of {subject}")),
            body_text: Some(format!("Full body of {subject}")),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: None,
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    // -----------------------------------------------------------------------
    // Test 1: Sources detection from List-Unsubscribe header
    // -----------------------------------------------------------------------
    #[test]
    fn test_sources_detection_from_list_unsubscribe_header() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg1 = make_newsletter_msg(&account.id, "news@techcrunch.com", "TechCrunch", "TC Daily #100", 1710000000, "tc-100");
        let msg2 = make_newsletter_msg(&account.id, "news@techcrunch.com", "TechCrunch", "TC Daily #101", 1710086400, "tc-101");
        let msg3 = make_newsletter_msg(&account.id, "hello@morning.com", "Morning Brew", "Your Daily Brew", 1710100000, "brew-1");

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &msg3);

        // Query sources using the same logic as list_sources handler
        let mut stmt = conn
            .prepare(
                "SELECT from_address, COUNT(*) as cnt
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                   AND (raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions'))
                 GROUP BY from_address
                 ORDER BY cnt DESC",
            )
            .unwrap();

        let sources: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].0, "news@techcrunch.com");
        assert_eq!(sources[0].1, 2);
        assert_eq!(sources[1].0, "hello@morning.com");
        assert_eq!(sources[1].1, 1);
    }

    // -----------------------------------------------------------------------
    // Test 2: Sources detection from ai_category
    // -----------------------------------------------------------------------
    #[test]
    fn test_sources_detection_from_ai_category() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert a message without List-Unsubscribe but with ai_category = 'Newsletters'
        let msg = make_promo_msg(&account.id, "deals@store.com", "Big Store", "Weekly Deals", 1710000000, "deals-1");
        let id = InsertMessage::insert(&conn, &msg).unwrap();

        // Set ai_category directly
        conn.execute(
            "UPDATE messages SET ai_category = 'Newsletters' WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap();

        // Also insert one with 'Promotions'
        let msg2 = make_promo_msg(&account.id, "offers@shop.com", "Shop Co", "50% Off", 1710086400, "shop-1");
        let id2 = InsertMessage::insert(&conn, &msg2).unwrap();
        conn.execute(
            "UPDATE messages SET ai_category = 'Promotions' WHERE id = ?1",
            rusqlite::params![id2],
        )
        .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT from_address, COUNT(*) as cnt
                 FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                   AND (raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions'))
                 GROUP BY from_address
                 ORDER BY cnt DESC",
            )
            .unwrap();

        let sources: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, Option<String>>(0)?.unwrap_or_default(), row.get(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(sources.len(), 2);
        // Both senders detected via ai_category
        let senders: Vec<String> = sources.iter().map(|(s, _)| s.clone()).collect();
        assert!(senders.contains(&"deals@store.com".to_string()));
        assert!(senders.contains(&"offers@shop.com".to_string()));
    }

    // -----------------------------------------------------------------------
    // Test 3: Preview returns correct messages
    // -----------------------------------------------------------------------
    #[test]
    fn test_preview_returns_correct_messages() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert newsletter messages
        let msg1 = make_newsletter_msg(&account.id, "news@example.com", "News", "News #1", 1710000000, "news-1");
        let msg2 = make_newsletter_msg(&account.id, "news@example.com", "News", "News #2", 1710086400, "news-2");

        // Insert a regular message (should not appear)
        let regular = make_regular_msg(&account.id, "friend@example.com", "Hello!", 1710050000, "reg-1");

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &regular);

        let (sql, params) = build_newsletter_query(None, None, &None, None);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let results: Vec<PreviewMessage> = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(PreviewMessage {
                    id: row.get(0)?,
                    from: row.get(1)?,
                    subject: row.get(3)?,
                    date: row.get(5)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(results.len(), 2);
        // All results should be from the newsletter sender
        for msg in &results {
            assert_eq!(msg.from.as_deref(), Some("news@example.com"));
        }
    }

    // -----------------------------------------------------------------------
    // Test 4: Digest generation stores in DB
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_stored_in_db() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();

        let digest_id = uuid::Uuid::new_v4().to_string();
        let message_ids = serde_json::to_string(&vec!["msg-1", "msg-2", "msg-3"]).unwrap();
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO newsletter_digests (id, title, summary, message_ids, source_count, message_count, date_from, date_to, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![digest_id, "Test Digest", "Summary content here", message_ids, 2, 3, 1710000000_i64, 1710100000_i64, now],
        )
        .unwrap();

        let stored: (String, String, String, i64, i64) = conn
            .query_row(
                "SELECT title, summary, message_ids, source_count, message_count FROM newsletter_digests WHERE id = ?1",
                rusqlite::params![digest_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .unwrap();

        assert_eq!(stored.0, "Test Digest");
        assert_eq!(stored.1, "Summary content here");
        assert!(stored.2.contains("msg-1"));
        assert_eq!(stored.3, 2);
        assert_eq!(stored.4, 3);
    }

    // -----------------------------------------------------------------------
    // Test 5: Digest history retrieval
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_history_retrieval() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();

        // Insert several digests
        for i in 0..5 {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO newsletter_digests (id, title, summary, message_ids, source_count, message_count, created_at)
                 VALUES (?1, ?2, ?3, '[]', 1, 2, ?4)",
                rusqlite::params![id, format!("Digest {i}"), format!("Summary {i}"), 1710000000 + i * 86400],
            )
            .unwrap();
        }

        // Retrieve with limit
        let mut stmt = conn
            .prepare(
                "SELECT id, title, summary, source_count, message_count, date_from, date_to, created_at
                 FROM newsletter_digests ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )
            .unwrap();

        let digests: Vec<DigestHistoryEntry> = stmt
            .query_map(rusqlite::params![3, 0], |row| {
                Ok(DigestHistoryEntry {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    summary: row.get(2)?,
                    source_count: row.get(3)?,
                    message_count: row.get(4)?,
                    date_from: row.get(5)?,
                    date_to: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(digests.len(), 3);
        // Most recent first
        assert_eq!(digests[0].title, "Digest 4");
        assert_eq!(digests[1].title, "Digest 3");
        assert_eq!(digests[2].title, "Digest 2");
    }

    // -----------------------------------------------------------------------
    // Test 6: Date range filtering
    // -----------------------------------------------------------------------
    #[test]
    fn test_date_range_filtering() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert messages across different dates
        let msg1 = make_newsletter_msg(&account.id, "news@example.com", "News", "Old News", 1709000000, "old-1");
        let msg2 = make_newsletter_msg(&account.id, "news@example.com", "News", "Recent News", 1710000000, "recent-1");
        let msg3 = make_newsletter_msg(&account.id, "news@example.com", "News", "Latest News", 1711000000, "latest-1");

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &msg3);

        // Filter to only include messages in the middle range
        let (sql, params) = build_newsletter_query(
            Some(1709500000),
            Some(1710500000),
            &None,
            None,
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let count: usize = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .count();

        assert_eq!(count, 1); // Only "Recent News" should match
    }

    // -----------------------------------------------------------------------
    // Test 7: Sender filtering
    // -----------------------------------------------------------------------
    #[test]
    fn test_sender_filtering() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg1 = make_newsletter_msg(&account.id, "news@alpha.com", "Alpha News", "Alpha Update", 1710000000, "alpha-1");
        let msg2 = make_newsletter_msg(&account.id, "news@beta.com", "Beta News", "Beta Update", 1710000000, "beta-1");
        let msg3 = make_newsletter_msg(&account.id, "news@gamma.com", "Gamma News", "Gamma Update", 1710000000, "gamma-1");

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &msg3);

        // Filter to only alpha and gamma
        let senders = Some(vec!["news@alpha.com".to_string(), "news@gamma.com".to_string()]);
        let (sql, params) = build_newsletter_query(None, None, &senders, None);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let results: Vec<String> = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, Option<String>>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|s| s)
            .collect();

        assert_eq!(results.len(), 2);
        assert!(results.contains(&"news@alpha.com".to_string()));
        assert!(results.contains(&"news@gamma.com".to_string()));
        assert!(!results.contains(&"news@beta.com".to_string()));
    }

    // -----------------------------------------------------------------------
    // Test 8: Empty inbox returns empty results
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_inbox_returns_empty_results() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();

        // No messages inserted at all
        let (sql, params) = build_newsletter_query(None, None, &None, None);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let count: usize = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .count();

        assert_eq!(count, 0);

        // Sources should also be empty
        let source_count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT from_address) FROM messages
                 WHERE is_deleted = 0 AND is_draft = 0
                   AND (raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions'))",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 0);
    }

    // -----------------------------------------------------------------------
    // Test 9: max_emails limit
    // -----------------------------------------------------------------------
    #[test]
    fn test_max_emails_limit() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert 10 newsletter messages
        for i in 0..10 {
            let msg = make_newsletter_msg(
                &account.id,
                "news@example.com",
                "News",
                &format!("Newsletter #{i}"),
                1710000000 + i * 3600,
                &format!("limit-{i}"),
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Limit to 3
        let (sql, params) = build_newsletter_query(None, None, &None, Some(3));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let count: usize = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .count();

        assert_eq!(count, 3);
    }

    // -----------------------------------------------------------------------
    // Test 10: max_emails capped at 500
    // -----------------------------------------------------------------------
    #[test]
    fn test_max_emails_capped_at_500() {
        let (sql, _) = build_newsletter_query(None, None, &None, Some(1000));
        // The SQL should contain LIMIT 500, not 1000
        assert!(sql.contains("LIMIT 500"));
    }

    // -----------------------------------------------------------------------
    // Test 11: Regular emails excluded from newsletter query
    // -----------------------------------------------------------------------
    #[test]
    fn test_regular_emails_excluded() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert regular email (no List-Unsubscribe, no newsletter ai_category)
        let regular = make_regular_msg(&account.id, "friend@example.com", "Lunch tomorrow?", 1710000000, "excl-1");
        InsertMessage::insert(&conn, &regular);

        // Insert a newsletter
        let newsletter = make_newsletter_msg(&account.id, "news@example.com", "News", "Daily Brief", 1710000000, "excl-2");
        InsertMessage::insert(&conn, &newsletter);

        let (sql, params) = build_newsletter_query(None, None, &None, None);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let results: Vec<String> = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, Option<String>>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .filter_map(|s| s)
            .collect();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "news@example.com");
    }

    // -----------------------------------------------------------------------
    // Test 12: Deleted and draft messages excluded
    // -----------------------------------------------------------------------
    #[test]
    fn test_deleted_and_draft_messages_excluded() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert a newsletter then soft-delete it
        let msg = make_newsletter_msg(&account.id, "news@example.com", "News", "Deleted Newsletter", 1710000000, "del-1");
        let id = InsertMessage::insert(&conn, &msg).unwrap();
        conn.execute("UPDATE messages SET is_deleted = 1 WHERE id = ?1", rusqlite::params![id]).unwrap();

        // Insert a draft newsletter
        let mut draft = make_newsletter_msg(&account.id, "news@example.com", "News", "Draft Newsletter", 1710000000, "draft-1");
        draft.is_draft = true;
        InsertMessage::insert(&conn, &draft);

        let (sql, params) = build_newsletter_query(None, None, &None, None);
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let count: usize = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .count();

        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // Test 13: Digest history pagination
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_history_pagination() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();

        for i in 0..10 {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO newsletter_digests (id, title, summary, message_ids, source_count, message_count, created_at)
                 VALUES (?1, ?2, ?3, '[]', 1, 1, ?4)",
                rusqlite::params![id, format!("Page Digest {i}"), format!("Summary {i}"), 1710000000 + i * 86400],
            )
            .unwrap();
        }

        // Page 1 (offset 0, limit 3)
        let mut stmt = conn
            .prepare(
                "SELECT title FROM newsletter_digests ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )
            .unwrap();
        let page1: Vec<String> = stmt
            .query_map(rusqlite::params![3, 0], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(page1.len(), 3);
        assert_eq!(page1[0], "Page Digest 9");

        // Page 2 (offset 3, limit 3)
        let page2: Vec<String> = stmt
            .query_map(rusqlite::params![3, 3], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(page2.len(), 3);
        assert_eq!(page2[0], "Page Digest 6");
    }

    // -----------------------------------------------------------------------
    // Test 14: Build newsletter query with all filters combined
    // -----------------------------------------------------------------------
    #[test]
    fn test_combined_filters() {
        let pool = setup_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Insert messages with different dates and senders
        let msg1 = make_newsletter_msg(&account.id, "a@example.com", "A", "A1", 1709000000, "combo-a1");
        let msg2 = make_newsletter_msg(&account.id, "a@example.com", "A", "A2", 1710000000, "combo-a2");
        let msg3 = make_newsletter_msg(&account.id, "b@example.com", "B", "B1", 1710000000, "combo-b1");
        let msg4 = make_newsletter_msg(&account.id, "a@example.com", "A", "A3", 1711000000, "combo-a3");

        InsertMessage::insert(&conn, &msg1);
        InsertMessage::insert(&conn, &msg2);
        InsertMessage::insert(&conn, &msg3);
        InsertMessage::insert(&conn, &msg4);

        // Filter: sender = a@example.com, date range = 1709500000..1710500000, max 10
        let senders = Some(vec!["a@example.com".to_string()]);
        let (sql, params) = build_newsletter_query(Some(1709500000), Some(1710500000), &senders, Some(10));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).unwrap();
        let results: Vec<(String, Option<String>)> = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        // Only msg2 should match (a@example.com within date range)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.as_deref(), Some("a@example.com"));
    }
}
