use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivePattern {
    pub id: String,
    pub pattern_type: String,
    pub pattern_value: String,
    pub confidence: f64,
    pub match_count: i64,
    pub total_from_sender: i64,
    pub archive_rate: f64,
    pub is_active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    pub message_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SuggestResponse {
    pub suggestions: Vec<Suggestion>,
}

#[derive(Debug, Serialize)]
pub struct Suggestion {
    pub message_id: String,
    pub pattern_id: String,
    pub reason: String,
    pub confidence: f64,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePatternRequest {
    pub is_active: Option<bool>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ComputeResponse {
    pub patterns_created: usize,
    pub patterns_updated: usize,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    pub updated: bool,
}

// --- Thresholds ---

const MIN_ARCHIVE_RATE: f64 = 0.8;
const MIN_MATCH_COUNT: i64 = 5;

// --- Handlers ---

/// POST /api/ai/archive-patterns/compute
/// Analyze archived messages to detect sender and category patterns.
pub async fn compute_patterns(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let result = compute_patterns_impl(&conn).map_err(|e| {
        tracing::error!("Failed to compute archive patterns: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(result))
}

/// GET /api/ai/archive-patterns
/// List all detected archive patterns.
pub async fn list_patterns(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ArchivePattern>>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut stmt = conn
        .prepare(
            "SELECT id, pattern_type, pattern_value, confidence, match_count,
                    total_from_sender, archive_rate, is_active, created_at, updated_at
             FROM archive_patterns
             ORDER BY confidence DESC, match_count DESC",
        )
        .map_err(|e| {
            tracing::error!("Failed to prepare list patterns query: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let patterns: Vec<ArchivePattern> = stmt
        .query_map([], |row| {
            Ok(ArchivePattern {
                id: row.get(0)?,
                pattern_type: row.get(1)?,
                pattern_value: row.get(2)?,
                confidence: row.get(3)?,
                match_count: row.get(4)?,
                total_from_sender: row.get(5)?,
                archive_rate: row.get(6)?,
                is_active: row.get::<_, i64>(7)? != 0,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| {
            tracing::error!("Failed to query archive patterns: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(patterns))
}

/// DELETE /api/ai/archive-patterns/{id}
/// Delete a specific pattern (user opt-out).
pub async fn delete_pattern(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let rows = conn
        .execute(
            "DELETE FROM archive_patterns WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| {
            tracing::error!("Failed to delete archive pattern: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(DeleteResponse { deleted: true }))
}

/// PUT /api/ai/archive-patterns/{id}
/// Update pattern (enable/disable, adjust threshold).
pub async fn update_pattern(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePatternRequest>,
) -> Result<Json<UpdateResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Verify pattern exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM archive_patterns WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(active) = req.is_active {
        updates.push(format!("is_active = ?{}", params.len() + 1));
        params.push(Box::new(active as i64));
    }

    if let Some(conf) = req.confidence {
        if !(0.0..=1.0).contains(&conf) {
            return Err(StatusCode::BAD_REQUEST);
        }
        updates.push(format!("confidence = ?{}", params.len() + 1));
        params.push(Box::new(conf));
    }

    if updates.is_empty() {
        return Ok(Json(UpdateResponse { updated: false }));
    }

    updates.push(format!("updated_at = unixepoch()"));

    let id_idx = params.len() + 1;
    params.push(Box::new(id));

    let sql = format!(
        "UPDATE archive_patterns SET {} WHERE id = ?{}",
        updates.join(", "),
        id_idx
    );

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = conn
        .execute(&sql, param_refs.as_slice())
        .map_err(|e| {
            tracing::error!("Failed to update archive pattern: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(UpdateResponse { updated: rows > 0 }))
}

/// POST /api/ai/archive-patterns/suggest
/// Given message IDs, return which ones match active archive patterns.
pub async fn suggest_archive(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SuggestRequest>,
) -> Result<Json<SuggestResponse>, StatusCode> {
    let conn = state.db.get().map_err(|e| {
        tracing::error!("DB pool error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let suggestions = suggest_archive_impl(&conn, &req.message_ids).map_err(|e| {
        tracing::error!("Failed to suggest archive patterns: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(SuggestResponse { suggestions }))
}

// --- Core logic (testable without HTTP) ---

fn compute_patterns_impl(
    conn: &rusqlite::Connection,
) -> Result<ComputeResponse, rusqlite::Error> {
    let mut created = 0usize;
    let mut updated = 0usize;

    // --- Sender-based patterns ---
    // For each sender: count archived messages and total messages, compute archive_rate.
    {
        let mut stmt = conn.prepare(
            "SELECT from_address,
                    SUM(CASE WHEN folder = 'Archive' THEN 1 ELSE 0 END) as archived_count,
                    COUNT(*) as total_count
             FROM messages
             WHERE from_address IS NOT NULL AND is_deleted = 0 AND is_draft = 0
             GROUP BY from_address
             HAVING archived_count >= ?1",
        )?;

        let rows: Vec<(String, i64, i64)> = stmt
            .query_map(rusqlite::params![MIN_MATCH_COUNT], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (sender, archived_count, total_count) in rows {
            let rate = archived_count as f64 / total_count as f64;
            if rate >= MIN_ARCHIVE_RATE {
                let (c, u) = upsert_pattern(
                    conn,
                    "sender",
                    &sender,
                    rate,
                    archived_count,
                    total_count,
                    rate,
                )?;
                created += c;
                updated += u;
            }
        }
    }

    // --- Category-based patterns ---
    // For each AI category: count archived vs total, create pattern if threshold met.
    {
        let mut stmt = conn.prepare(
            "SELECT ai_category,
                    SUM(CASE WHEN folder = 'Archive' THEN 1 ELSE 0 END) as archived_count,
                    COUNT(*) as total_count
             FROM messages
             WHERE ai_category IS NOT NULL AND is_deleted = 0 AND is_draft = 0
             GROUP BY ai_category
             HAVING archived_count >= ?1",
        )?;

        let rows: Vec<(String, i64, i64)> = stmt
            .query_map(rusqlite::params![MIN_MATCH_COUNT], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (category, archived_count, total_count) in rows {
            let rate = archived_count as f64 / total_count as f64;
            if rate >= MIN_ARCHIVE_RATE {
                let (c, u) = upsert_pattern(
                    conn,
                    "category",
                    &category,
                    rate,
                    archived_count,
                    total_count,
                    rate,
                )?;
                created += c;
                updated += u;
            }
        }
    }

    // --- Sender+Category combination patterns ---
    // Detect patterns where a specific sender's emails in a specific category are always archived.
    {
        let mut stmt = conn.prepare(
            "SELECT from_address || '::' || ai_category as combo,
                    SUM(CASE WHEN folder = 'Archive' THEN 1 ELSE 0 END) as archived_count,
                    COUNT(*) as total_count
             FROM messages
             WHERE from_address IS NOT NULL AND ai_category IS NOT NULL
                   AND is_deleted = 0 AND is_draft = 0
             GROUP BY combo
             HAVING archived_count >= ?1",
        )?;

        let rows: Vec<(String, i64, i64)> = stmt
            .query_map(rusqlite::params![MIN_MATCH_COUNT], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (combo, archived_count, total_count) in rows {
            let rate = archived_count as f64 / total_count as f64;
            if rate >= MIN_ARCHIVE_RATE {
                let (c, u) = upsert_pattern(
                    conn,
                    "sender_category",
                    &combo,
                    rate,
                    archived_count,
                    total_count,
                    rate,
                )?;
                created += c;
                updated += u;
            }
        }
    }

    Ok(ComputeResponse {
        patterns_created: created,
        patterns_updated: updated,
    })
}

/// Upsert a pattern: insert if new, update if exists.
/// Returns (created_count, updated_count) — one of them will be 1, the other 0.
fn upsert_pattern(
    conn: &rusqlite::Connection,
    pattern_type: &str,
    pattern_value: &str,
    confidence: f64,
    match_count: i64,
    total_from_sender: i64,
    archive_rate: f64,
) -> Result<(usize, usize), rusqlite::Error> {
    let id = uuid::Uuid::new_v4().to_string();

    let rows = conn.execute(
        "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(pattern_type, pattern_value) DO UPDATE SET
             confidence = excluded.confidence,
             match_count = excluded.match_count,
             total_from_sender = excluded.total_from_sender,
             archive_rate = excluded.archive_rate,
             updated_at = unixepoch()",
        rusqlite::params![id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate],
    )?;

    // Check if this was an insert or update by seeing if the id we generated is in the table
    let found: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM archive_patterns WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if rows > 0 && found {
        Ok((1, 0)) // created
    } else if rows > 0 {
        Ok((0, 1)) // updated
    } else {
        Ok((0, 0))
    }
}

fn suggest_archive_impl(
    conn: &rusqlite::Connection,
    message_ids: &[String],
) -> Result<Vec<Suggestion>, rusqlite::Error> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Load all active patterns
    let mut stmt = conn.prepare(
        "SELECT id, pattern_type, pattern_value, confidence
         FROM archive_patterns
         WHERE is_active = 1
         ORDER BY confidence DESC",
    )?;

    let patterns: Vec<(String, String, String, f64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    if patterns.is_empty() {
        return Ok(Vec::new());
    }

    let mut suggestions = Vec::new();

    for msg_id in message_ids {
        // Fetch message details
        let msg: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT from_address, ai_category FROM messages WHERE id = ?1 AND is_deleted = 0",
                rusqlite::params![msg_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        let (from_address, ai_category) = match msg {
            Some(m) => m,
            None => continue,
        };

        // Check each pattern — use the first (highest confidence) match
        for (pat_id, pat_type, pat_value, confidence) in &patterns {
            let matched = match pat_type.as_str() {
                "sender" => from_address.as_deref() == Some(pat_value.as_str()),
                "category" => ai_category.as_deref() == Some(pat_value.as_str()),
                "sender_category" => {
                    // pat_value is "sender::category"
                    if let Some((sender, cat)) = pat_value.split_once("::") {
                        from_address.as_deref() == Some(sender)
                            && ai_category.as_deref() == Some(cat)
                    } else {
                        false
                    }
                }
                "subject_pattern" => {
                    // Future: regex or keyword matching on subject
                    false
                }
                _ => false,
            };

            if matched {
                let reason = match pat_type.as_str() {
                    "sender" => format!("Emails from {} are typically archived", pat_value),
                    "category" => format!("Emails in category '{}' are typically archived", pat_value),
                    "sender_category" => {
                        let parts: Vec<&str> = pat_value.splitn(2, "::").collect();
                        if parts.len() == 2 {
                            format!(
                                "Emails from {} in category '{}' are typically archived",
                                parts[0], parts[1]
                            )
                        } else {
                            format!("Matches archive pattern: {}", pat_value)
                        }
                    }
                    _ => format!("Matches archive pattern: {}", pat_value),
                };

                suggestions.push(Suggestion {
                    message_id: msg_id.clone(),
                    pattern_id: pat_id.clone(),
                    reason,
                    confidence: *confidence,
                });
                break; // Only use the best (first) match per message
            }
        }
    }

    Ok(suggestions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "archive-test@example.com".to_string(),
            display_name: Some("Archive Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("archive-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        folder: &str,
        from_address: &str,
        subject: &str,
        _category: Option<&str>,
        uid: i64,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}-{}@example.com>", subject.replace(' ', "-"), uid)),
            thread_id: None,
            folder: folder.to_string(),
            from_address: Some(from_address.to_string()),
            from_name: Some("Test Sender".to_string()),
            to_addresses: Some(r#"["archive-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000 + uid),
            snippet: Some("Preview text...".to_string()),
            body_text: Some("Full body text".to_string()),
            body_html: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(uid),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    fn ensure_archive_patterns_table(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS archive_patterns (
                id TEXT PRIMARY KEY,
                pattern_type TEXT NOT NULL,
                pattern_value TEXT NOT NULL,
                confidence REAL NOT NULL DEFAULT 0.0,
                match_count INTEGER NOT NULL DEFAULT 0,
                total_from_sender INTEGER NOT NULL DEFAULT 0,
                archive_rate REAL NOT NULL DEFAULT 0.0,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(pattern_type, pattern_value)
            );",
        )
        .unwrap();
    }

    fn set_ai_category(conn: &rusqlite::Connection, msg_id: &str, category: &str) {
        conn.execute(
            "UPDATE messages SET ai_category = ?1 WHERE id = ?2",
            rusqlite::params![category, msg_id],
        )
        .unwrap();
    }

    #[test]
    fn test_compute_finds_sender_patterns() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 6 archived messages from same sender (above threshold)
        for i in 0..6 {
            let msg = make_message(
                &account.id,
                "Archive",
                "newsletter@spam.com",
                &format!("Newsletter {i}"),
                None,
                100 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Insert 1 non-archived message from same sender (total = 7, archive_rate = 6/7 = 0.857)
        let inbox_msg = make_message(
            &account.id,
            "INBOX",
            "newsletter@spam.com",
            "Newsletter latest",
            None,
            200,
        );
        InsertMessage::insert(&conn, &inbox_msg);

        let result = compute_patterns_impl(&conn).unwrap();
        assert!(result.patterns_created > 0, "Should have created at least one sender pattern");

        // Verify pattern was stored
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE pattern_type = 'sender' AND pattern_value = 'newsletter@spam.com'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_compute_finds_category_patterns() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 6 archived "Promotions" messages from various senders
        for i in 0..6 {
            let msg = make_message(
                &account.id,
                "Archive",
                &format!("promo{i}@store.com"),
                &format!("Sale {i}"),
                None,
                300 + i,
            );
            let msg_id = InsertMessage::insert(&conn, &msg).unwrap();
            set_ai_category(&conn, &msg_id, "Promotions");
        }

        let result = compute_patterns_impl(&conn).unwrap();
        assert!(result.patterns_created > 0);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE pattern_type = 'category' AND pattern_value = 'Promotions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_suggest_matches_sender_pattern() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Manually insert a sender pattern
        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
             VALUES ('pat-1', 'sender', 'spam@example.com', 0.95, 10, 11, 0.909)",
            [],
        )
        .unwrap();

        // Insert a message from that sender
        let msg = make_message(
            &account.id,
            "INBOX",
            "spam@example.com",
            "Buy now!",
            None,
            400,
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let suggestions = suggest_archive_impl(&conn, &[msg_id.clone()]).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].message_id, msg_id);
        assert_eq!(suggestions[0].pattern_id, "pat-1");
        assert!(suggestions[0].confidence > 0.9);
        assert!(suggestions[0].reason.contains("spam@example.com"));
    }

    #[test]
    fn test_suggest_matches_category_pattern() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert a category pattern
        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
             VALUES ('pat-cat', 'category', 'Promotions', 0.88, 20, 22, 0.909)",
            [],
        )
        .unwrap();

        // Insert a message with that category
        let msg = make_message(
            &account.id,
            "INBOX",
            "store@shop.com",
            "Big Sale",
            None,
            500,
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();
        set_ai_category(&conn, &msg_id, "Promotions");

        let suggestions = suggest_archive_impl(&conn, &[msg_id.clone()]).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].pattern_id, "pat-cat");
        assert!(suggestions[0].reason.contains("Promotions"));
    }

    #[test]
    fn test_suggest_no_match() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert a sender pattern for a different sender
        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
             VALUES ('pat-other', 'sender', 'other@example.com', 0.95, 10, 10, 1.0)",
            [],
        )
        .unwrap();

        // Insert a message from a different sender
        let msg = make_message(
            &account.id,
            "INBOX",
            "important@work.com",
            "Critical update",
            None,
            600,
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let suggestions = suggest_archive_impl(&conn, &[msg_id]).unwrap();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_delete_pattern() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);

        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
             VALUES ('del-1', 'sender', 'delete-me@example.com', 0.9, 8, 9, 0.889)",
            [],
        )
        .unwrap();

        // Verify it exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE id = 'del-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete it
        let rows = conn
            .execute(
                "DELETE FROM archive_patterns WHERE id = 'del-1'",
                [],
            )
            .unwrap();
        assert_eq!(rows, 1);

        // Verify it's gone
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE id = 'del-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_update_enable_disable() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);

        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate, is_active)
             VALUES ('upd-1', 'sender', 'toggle@example.com', 0.9, 8, 9, 0.889, 1)",
            [],
        )
        .unwrap();

        // Disable
        conn.execute(
            "UPDATE archive_patterns SET is_active = 0, updated_at = unixepoch() WHERE id = 'upd-1'",
            [],
        )
        .unwrap();

        let active: i64 = conn
            .query_row(
                "SELECT is_active FROM archive_patterns WHERE id = 'upd-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active, 0);

        // Suggest should not match disabled pattern
        // (need a message to test against)
        let account = create_test_account(&conn);
        let msg = make_message(
            &account.id,
            "INBOX",
            "toggle@example.com",
            "Test msg",
            None,
            700,
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();

        let suggestions = suggest_archive_impl(&conn, &[msg_id.clone()]).unwrap();
        assert!(suggestions.is_empty(), "Disabled pattern should not match");

        // Re-enable
        conn.execute(
            "UPDATE archive_patterns SET is_active = 1, updated_at = unixepoch() WHERE id = 'upd-1'",
            [],
        )
        .unwrap();

        let suggestions = suggest_archive_impl(&conn, &[msg_id]).unwrap();
        assert_eq!(suggestions.len(), 1, "Re-enabled pattern should match");
    }

    #[test]
    fn test_empty_inbox_returns_empty_patterns() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);

        // No messages at all — compute should produce no patterns
        let result = compute_patterns_impl(&conn).unwrap();
        assert_eq!(result.patterns_created, 0);
        assert_eq!(result.patterns_updated, 0);

        // List should be empty
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_low_confidence_patterns_not_created() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 3 archived messages (below MIN_MATCH_COUNT = 5)
        for i in 0..3 {
            let msg = make_message(
                &account.id,
                "Archive",
                "low-vol@example.com",
                &format!("Low vol {i}"),
                None,
                800 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let result = compute_patterns_impl(&conn).unwrap();
        assert_eq!(result.patterns_created, 0, "Should not create pattern with < 5 archived");
    }

    #[test]
    fn test_low_archive_rate_not_created() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 5 archived, 10 in inbox (rate = 5/15 = 0.33 < 0.8)
        for i in 0..5 {
            let msg = make_message(
                &account.id,
                "Archive",
                "mixed@example.com",
                &format!("Archived {i}"),
                None,
                900 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }
        for i in 0..10 {
            let msg = make_message(
                &account.id,
                "INBOX",
                "mixed@example.com",
                &format!("Inbox {i}"),
                None,
                950 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let result = compute_patterns_impl(&conn).unwrap();
        assert_eq!(result.patterns_created, 0, "Should not create pattern with archive_rate < 0.8");
    }

    #[test]
    fn test_pattern_deduplication_upsert() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 6 archived messages
        for i in 0..6 {
            let msg = make_message(
                &account.id,
                "Archive",
                "repeat@example.com",
                &format!("Repeat {i}"),
                None,
                1000 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }

        // Compute once
        let result1 = compute_patterns_impl(&conn).unwrap();
        assert_eq!(result1.patterns_created, 1);

        // Insert 2 more archived messages and compute again
        for i in 6..8 {
            let msg = make_message(
                &account.id,
                "Archive",
                "repeat@example.com",
                &format!("Repeat {i}"),
                None,
                1000 + i,
            );
            InsertMessage::insert(&conn, &msg);
        }

        let result2 = compute_patterns_impl(&conn).unwrap();
        assert_eq!(result2.patterns_updated, 1, "Should update existing pattern");
        assert_eq!(result2.patterns_created, 0, "Should not create duplicate");

        // Only one pattern in table
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE pattern_type = 'sender' AND pattern_value = 'repeat@example.com'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_suggest_with_empty_message_ids() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);

        let suggestions = suggest_archive_impl(&conn, &[]).unwrap();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_suggest_sender_category_combo() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert a sender_category pattern
        conn.execute(
            "INSERT INTO archive_patterns (id, pattern_type, pattern_value, confidence, match_count, total_from_sender, archive_rate)
             VALUES ('pat-sc', 'sender_category', 'store@shop.com::Promotions', 0.92, 15, 16, 0.9375)",
            [],
        )
        .unwrap();

        // Insert a message matching both sender and category
        let msg = make_message(
            &account.id,
            "INBOX",
            "store@shop.com",
            "Flash sale",
            None,
            1100,
        );
        let msg_id = InsertMessage::insert(&conn, &msg).unwrap();
        set_ai_category(&conn, &msg_id, "Promotions");

        let suggestions = suggest_archive_impl(&conn, &[msg_id.clone()]).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].pattern_id, "pat-sc");
        assert!(suggestions[0].reason.contains("store@shop.com"));
        assert!(suggestions[0].reason.contains("Promotions"));
    }

    #[test]
    fn test_compute_sender_category_patterns() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        ensure_archive_patterns_table(&conn);
        let account = create_test_account(&conn);

        // Insert 6 archived messages from same sender in same category
        for i in 0..6 {
            let msg = make_message(
                &account.id,
                "Archive",
                "alerts@service.com",
                &format!("Alert {i}"),
                None,
                1200 + i,
            );
            let msg_id = InsertMessage::insert(&conn, &msg).unwrap();
            set_ai_category(&conn, &msg_id, "Updates");
        }

        let result = compute_patterns_impl(&conn).unwrap();
        assert!(result.patterns_created > 0);

        // Should have a sender_category pattern
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM archive_patterns WHERE pattern_type = 'sender_category'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count >= 1);
    }
}
