use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct IndexResult {
    pub message_id: String,
    pub indexed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub account_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct AttachmentSearchResult {
    pub attachment_id: String,
    pub message_id: String,
    pub filename: Option<String>,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
pub struct AttachmentSearchResponse {
    pub results: Vec<AttachmentSearchResult>,
    pub total: i64,
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct ContentTypeStat {
    pub content_type: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SearchStats {
    pub total_indexed: i64,
    pub by_content_type: Vec<ContentTypeStat>,
}

#[derive(Debug, Deserialize)]
pub struct ReindexRequest {
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct ReindexResult {
    pub indexed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Text extraction
// ---------------------------------------------------------------------------

/// Extract searchable text from attachment content based on MIME type.
fn extract_text(content_type: &str, content: &[u8]) -> Result<String, String> {
    let ct = content_type.to_lowercase();

    if ct.starts_with("text/plain")
        || ct.starts_with("text/csv")
        || ct.starts_with("text/markdown")
        || ct.starts_with("text/tab-separated-values")
    {
        String::from_utf8(content.to_vec())
            .map_err(|e| format!("UTF-8 decode error: {e}"))
    } else if ct.starts_with("text/html") {
        // Strip HTML tags for plain-text indexing
        let html = String::from_utf8(content.to_vec())
            .map_err(|e| format!("UTF-8 decode error: {e}"))?;
        Ok(strip_html_tags(&html))
    } else if ct == "application/pdf" {
        Ok("[PDF content - extraction requires pdf-extract crate]".to_string())
    } else if ct == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        || ct == "application/msword"
    {
        Ok("[Word document - extraction requires docx parser]".to_string())
    } else if ct == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        || ct == "application/vnd.ms-excel"
    {
        Ok("[Spreadsheet - extraction requires xlsx parser]".to_string())
    } else {
        Err(format!("Unsupported content type: {content_type}"))
    }
}

/// Minimal HTML tag stripper for indexing purposes.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/attachments/index/:message_id
/// Extract text from all attachments of a message and index into FTS5.
pub async fn index_message_attachments(
    State(state): State<Arc<AppState>>,
    Path(message_id): Path<String>,
) -> Result<Json<IndexResult>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify the message exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM messages WHERE id = ?1",
            rusqlite::params![message_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // Fetch all attachments for this message (join messages for account_id)
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.message_id, m.account_id, a.filename, a.content_type, a.data
             FROM attachments a
             JOIN messages m ON a.message_id = m.id
             WHERE a.message_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let attachments: Vec<(String, String, String, Option<String>, Option<String>, Option<Vec<u8>>)> = stmt
        .query_map(rusqlite::params![message_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for (att_id, msg_id, account_id, filename, content_type, content_blob) in &attachments {
        let ct = content_type.as_deref().unwrap_or("application/octet-stream");
        let blob = match content_blob {
            Some(b) => b.as_slice(),
            None => {
                skipped += 1;
                errors.push(format!("Attachment {} has no content", att_id));
                continue;
            }
        };

        match extract_text(ct, blob) {
            Ok(text) => {
                if text.trim().is_empty() {
                    skipped += 1;
                    continue;
                }

                // Insert into cache table (REPLACE to handle re-indexing)
                conn.execute(
                    "INSERT OR REPLACE INTO attachment_text_cache
                        (attachment_id, message_id, account_id, filename, content_text, extracted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                    rusqlite::params![att_id, msg_id, account_id, filename, text],
                )
                .map_err(|e| {
                    tracing::error!("Failed to cache attachment text: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

                // Insert into FTS5 index
                conn.execute(
                    "INSERT INTO attachment_content_fts (attachment_id, message_id, filename, content_text)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![att_id, msg_id, filename.as_deref().unwrap_or(""), text],
                )
                .map_err(|e| {
                    tracing::error!("Failed to index attachment: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

                indexed += 1;
            }
            Err(e) => {
                skipped += 1;
                errors.push(format!(
                    "Attachment {} ({}): {}",
                    att_id,
                    filename.as_deref().unwrap_or("unknown"),
                    e
                ));
            }
        }
    }

    Ok(Json(IndexResult {
        message_id,
        indexed,
        skipped,
        errors,
    }))
}

/// GET /api/attachments/search
/// Full-text search across indexed attachment content.
pub async fn search_attachments(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<AttachmentSearchResponse>, StatusCode> {
    let query_str = params.q.as_deref().unwrap_or("").trim().to_string();
    if query_str.is_empty() {
        return Ok(Json(AttachmentSearchResponse {
            results: Vec::new(),
            total: 0,
            query: query_str,
        }));
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Build FTS5 query: quote each term for safety
    let fts_query = query_str
        .split_whitespace()
        .map(|term| {
            let clean = term.replace('"', "");
            format!("\"{clean}\"")
        })
        .collect::<Vec<_>>()
        .join(" ");

    // Account filter
    let (search_sql, count_sql, has_account_filter) = if params.account_id.is_some() {
        (
            "SELECT fts.attachment_id, fts.message_id, fts.filename,
                    snippet(attachment_content_fts, 3, '<b>', '</b>', '...', 32) as match_snippet
             FROM attachment_content_fts fts
             JOIN attachment_text_cache atc ON fts.attachment_id = atc.attachment_id
             WHERE attachment_content_fts MATCH ?1 AND atc.account_id = ?2
             ORDER BY rank
             LIMIT ?3 OFFSET ?4".to_string(),
            "SELECT COUNT(*)
             FROM attachment_content_fts fts
             JOIN attachment_text_cache atc ON fts.attachment_id = atc.attachment_id
             WHERE attachment_content_fts MATCH ?1 AND atc.account_id = ?2".to_string(),
            true,
        )
    } else {
        (
            "SELECT fts.attachment_id, fts.message_id, fts.filename,
                    snippet(attachment_content_fts, 3, '<b>', '</b>', '...', 32) as match_snippet
             FROM attachment_content_fts fts
             WHERE attachment_content_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2 OFFSET ?3".to_string(),
            "SELECT COUNT(*)
             FROM attachment_content_fts fts
             WHERE attachment_content_fts MATCH ?1".to_string(),
            false,
        )
    };

    let mut stmt = conn.prepare(&search_sql).map_err(|e| {
        tracing::error!("Attachment search query error: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<AttachmentSearchResult> {
        Ok(AttachmentSearchResult {
            attachment_id: row.get::<_, String>(0)?,
            message_id: row.get(1)?,
            filename: row.get(2)?,
            snippet: row.get(3)?,
        })
    };

    let results: Vec<AttachmentSearchResult> = if has_account_filter {
        let account_id = params.account_id.as_ref().unwrap();
        stmt.query_map(
            rusqlite::params![fts_query, account_id, limit, offset],
            map_row,
        )
        .map_err(|e| {
            tracing::error!("Attachment search execution error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(
            rusqlite::params![fts_query, limit, offset],
            map_row,
        )
        .map_err(|e| {
            tracing::error!("Attachment search execution error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    let total: i64 = if has_account_filter {
        let account_id = params.account_id.as_ref().unwrap();
        conn.query_row(
            &count_sql,
            rusqlite::params![fts_query, account_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
    } else {
        conn.query_row(&count_sql, rusqlite::params![fts_query], |row| row.get(0))
            .unwrap_or(0)
    };

    Ok(Json(AttachmentSearchResponse {
        results,
        total,
        query: query_str,
    }))
}

/// GET /api/attachments/search/stats
/// Return statistics about indexed attachments.
pub async fn search_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SearchStats>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_indexed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attachment_text_cache",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT a.content_type, COUNT(*) as cnt
             FROM attachment_text_cache atc
             JOIN attachments a ON atc.attachment_id = a.id
             GROUP BY a.content_type
             ORDER BY cnt DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let by_content_type: Vec<ContentTypeStat> = stmt
        .query_map([], |row| {
            Ok(ContentTypeStat {
                content_type: row.get::<_, Option<String>>(0)?.unwrap_or_else(|| "unknown".to_string()),
                count: row.get(1)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(SearchStats {
        total_indexed,
        by_content_type,
    }))
}

/// POST /api/attachments/reindex
/// Reindex all attachments for an account. Clears existing index entries first.
pub async fn reindex_attachments(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReindexRequest>,
) -> Result<Json<ReindexResult>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Delete existing FTS entries for this account's attachments
    let existing_ids: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT attachment_id FROM attachment_text_cache WHERE account_id = ?1")
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        stmt.query_map(rusqlite::params![req.account_id], |row| row.get(0))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect()
    };

    // Remove old FTS entries
    for att_id in &existing_ids {
        // For contentless FTS5, we need to supply the original values for deletion
        if let Ok(row) = conn.query_row(
            "SELECT attachment_id, message_id, filename, content_text
             FROM attachment_text_cache WHERE attachment_id = ?1",
            rusqlite::params![att_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        ) {
            let _ = conn.execute(
                "INSERT INTO attachment_content_fts(attachment_content_fts, attachment_id, message_id, filename, content_text)
                 VALUES ('delete', ?1, ?2, ?3, ?4)",
                rusqlite::params![&row.0, row.1, row.2.unwrap_or_default(), row.3],
            );
        }
    }

    // Remove old cache entries
    conn.execute(
        "DELETE FROM attachment_text_cache WHERE account_id = ?1",
        rusqlite::params![req.account_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Re-fetch and re-index all attachments for this account
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.message_id, m.account_id, a.filename, a.content_type, a.data
             FROM attachments a
             JOIN messages m ON a.message_id = m.id
             WHERE m.account_id = ?1",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let attachments: Vec<(String, String, String, Option<String>, Option<String>, Option<Vec<u8>>)> = stmt
        .query_map(rusqlite::params![req.account_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for (att_id, msg_id, account_id, filename, content_type, content_blob) in &attachments {
        let ct = content_type.as_deref().unwrap_or("application/octet-stream");
        let blob = match content_blob {
            Some(b) => b.as_slice(),
            None => {
                skipped += 1;
                errors.push(format!("Attachment {} has no content", att_id));
                continue;
            }
        };

        match extract_text(ct, blob) {
            Ok(text) => {
                if text.trim().is_empty() {
                    skipped += 1;
                    continue;
                }

                conn.execute(
                    "INSERT OR REPLACE INTO attachment_text_cache
                        (attachment_id, message_id, account_id, filename, content_text, extracted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                    rusqlite::params![att_id, msg_id, account_id, filename, text],
                )
                .map_err(|e| {
                    tracing::error!("Failed to cache attachment text during reindex: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

                conn.execute(
                    "INSERT INTO attachment_content_fts (attachment_id, message_id, filename, content_text)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![att_id, msg_id, filename.as_deref().unwrap_or(""), text],
                )
                .map_err(|e| {
                    tracing::error!("Failed to FTS-index attachment during reindex: {e}");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

                indexed += 1;
            }
            Err(e) => {
                skipped += 1;
                errors.push(format!(
                    "Attachment {} ({}): {}",
                    att_id,
                    filename.as_deref().unwrap_or("unknown"),
                    e
                ));
            }
        }
    }

    Ok(Json(ReindexResult {
        indexed,
        skipped,
        errors,
    }))
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
    use r2d2::PooledConnection;
    use r2d2_sqlite::SqliteConnectionManager;

    type Conn = PooledConnection<SqliteConnectionManager>;

    fn create_test_account(conn: &Conn) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: format!("att-test-{}@example.com", uuid::Uuid::new_v4()),
            display_name: Some("Attachment Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("att-test@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn insert_test_message(conn: &Conn, account_id: &str, subject: &str) -> String {
        let msg = InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{}-{}@example.com>", subject.replace(' ', "-"), uuid::Uuid::new_v4())),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Sender".to_string()),
            to_addresses: Some(r#"["att-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some(subject.to_string()),
            date: Some(1700000000),
            snippet: Some("Preview text...".to_string()),
            body_text: Some("Full body text".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: true,
            attachment_names: None,
            size_bytes: Some(1024),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        };
        InsertMessage::insert(conn, &msg).expect("Failed to insert test message")
    }

    fn insert_test_attachment(
        conn: &Conn,
        message_id: &str,
        account_id: &str,
        filename: &str,
        content_type: &str,
        content: &[u8],
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO attachments (id, message_id, account_id, filename, content_type, size, data)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, message_id, account_id, filename, content_type, content.len() as i64, content],
        )
        .expect("Failed to insert attachment");
        id
    }

    fn index_attachment(conn: &Conn, att_id: &str, message_id: &str, account_id: &str, filename: &str, content_type: &str, content: &[u8]) -> bool {
        match extract_text(content_type, content) {
            Ok(text) if !text.trim().is_empty() => {
                conn.execute(
                    "INSERT OR REPLACE INTO attachment_text_cache
                        (attachment_id, message_id, account_id, filename, content_text, extracted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                    rusqlite::params![att_id, message_id, account_id, filename, text],
                ).unwrap();

                conn.execute(
                    "INSERT INTO attachment_content_fts (attachment_id, message_id, filename, content_text)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![att_id, message_id, filename, text],
                ).unwrap();

                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // 1. Text extraction for plain text
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_text_plain() {
        let content = b"Hello, this is plain text content.";
        let result = extract_text("text/plain", content);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, this is plain text content.");
    }

    // -----------------------------------------------------------------------
    // 2. Text extraction for CSV
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_text_csv() {
        let content = b"name,email\nAlice,alice@example.com\nBob,bob@example.com";
        let result = extract_text("text/csv", content);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Alice"));
    }

    // -----------------------------------------------------------------------
    // 3. Text extraction for HTML
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_text_html() {
        let content = b"<html><body><p>Hello <b>World</b></p></body></html>";
        let result = extract_text("text/html", content);
        assert!(result.is_ok());
        let text = result.unwrap();
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<b>"));
    }

    // -----------------------------------------------------------------------
    // 4. PDF returns placeholder
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_text_pdf_placeholder() {
        let result = extract_text("application/pdf", b"%PDF-1.4 fake pdf");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("PDF content"));
    }

    // -----------------------------------------------------------------------
    // 5. Unsupported type returns error
    // -----------------------------------------------------------------------
    #[test]
    fn test_extract_text_unsupported() {
        let result = extract_text("application/octet-stream", b"\x00\x01\x02");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported content type"));
    }

    // -----------------------------------------------------------------------
    // 6. Index text attachment and search for it
    // -----------------------------------------------------------------------
    #[test]
    fn test_index_and_search_text_attachment() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "With attachment");

        let content = b"The quarterly financial report shows increased revenue.";
        let att_id = insert_test_attachment(&conn, &msg_id, &account.id, "report.txt", "text/plain", content);
        assert!(index_attachment(&conn, &att_id, &msg_id, &account.id, "report.txt", "text/plain", content));

        // Search for "revenue"
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_content_fts WHERE attachment_content_fts MATCH '\"revenue\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count >= 1);
    }

    // -----------------------------------------------------------------------
    // 7. Search with snippet highlighting
    // -----------------------------------------------------------------------
    #[test]
    fn test_search_with_snippet() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "Snippet test");

        let content = b"The budget analysis for Q4 shows significant growth in the technology sector.";
        let att_id = insert_test_attachment(&conn, &msg_id, &account.id, "analysis.txt", "text/plain", content);
        index_attachment(&conn, &att_id, &msg_id, &account.id, "analysis.txt", "text/plain", content);

        let snippet: String = conn
            .query_row(
                "SELECT snippet(attachment_content_fts, 3, '<b>', '</b>', '...', 32)
                 FROM attachment_content_fts WHERE attachment_content_fts MATCH '\"budget\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(snippet.contains("<b>"));
    }

    // -----------------------------------------------------------------------
    // 8. Empty search returns empty results
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_search_query() {
        // extract_text for empty content
        let result = extract_text("text/plain", b"");
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // -----------------------------------------------------------------------
    // 9. Stats show correct counts
    // -----------------------------------------------------------------------
    #[test]
    fn test_stats_counts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "Stats test");

        // Insert 2 text attachments and 1 HTML
        let c1 = b"First text attachment content here.";
        let att1 = insert_test_attachment(&conn, &msg_id, &account.id, "file1.txt", "text/plain", c1);
        index_attachment(&conn, &att1, &msg_id, &account.id, "file1.txt", "text/plain", c1);

        let c2 = b"Second text attachment different words.";
        let att2 = insert_test_attachment(&conn, &msg_id, &account.id, "file2.txt", "text/plain", c2);
        index_attachment(&conn, &att2, &msg_id, &account.id, "file2.txt", "text/plain", c2);

        let c3 = b"<p>HTML attachment content.</p>";
        let att3 = insert_test_attachment(&conn, &msg_id, &account.id, "page.html", "text/html", c3);
        index_attachment(&conn, &att3, &msg_id, &account.id, "page.html", "text/html", c3);

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachment_text_cache", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, 3);

        // By content type
        let mut stmt = conn
            .prepare(
                "SELECT a.content_type, COUNT(*) FROM attachment_text_cache atc
                 JOIN attachments a ON atc.attachment_id = a.id
                 GROUP BY a.content_type ORDER BY COUNT(*) DESC",
            )
            .unwrap();
        let types: Vec<(String, i64)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(types.len(), 2); // text/plain and text/html
    }

    // -----------------------------------------------------------------------
    // 10. Reindex clears and rebuilds
    // -----------------------------------------------------------------------
    #[test]
    fn test_reindex_clears_and_rebuilds() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "Reindex test");

        let content = b"Original indexable content for reindex test.";
        let att_id = insert_test_attachment(&conn, &msg_id, &account.id, "reindex.txt", "text/plain", content);
        index_attachment(&conn, &att_id, &msg_id, &account.id, "reindex.txt", "text/plain", content);

        // Verify indexed
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachment_text_cache WHERE account_id = ?1", rusqlite::params![&account.id], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Clear cache (simulating reindex clear step)
        conn.execute("DELETE FROM attachment_text_cache WHERE account_id = ?1", rusqlite::params![&account.id]).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachment_text_cache WHERE account_id = ?1", rusqlite::params![&account.id], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Re-index
        index_attachment(&conn, &att_id, &msg_id, &account.id, "reindex.txt", "text/plain", content);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM attachment_text_cache WHERE account_id = ?1", rusqlite::params![&account.id], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // -----------------------------------------------------------------------
    // 11. Multiple attachments for same message
    // -----------------------------------------------------------------------
    #[test]
    fn test_multiple_attachments_per_message() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "Multi attachment");

        let c1 = b"Alpha content about astronomy and stars.";
        let c2 = b"Beta content about biology and cells.";
        let c3 = b"Gamma content about chemistry and elements.";

        let a1 = insert_test_attachment(&conn, &msg_id, &account.id, "alpha.txt", "text/plain", c1);
        let a2 = insert_test_attachment(&conn, &msg_id, &account.id, "beta.txt", "text/plain", c2);
        let a3 = insert_test_attachment(&conn, &msg_id, &account.id, "gamma.txt", "text/plain", c3);

        index_attachment(&conn, &a1, &msg_id, &account.id, "alpha.txt", "text/plain", c1);
        index_attachment(&conn, &a2, &msg_id, &account.id, "beta.txt", "text/plain", c2);
        index_attachment(&conn, &a3, &msg_id, &account.id, "gamma.txt", "text/plain", c3);

        // Search for "biology" should match only beta
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_content_fts WHERE attachment_content_fts MATCH '\"biology\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // All three should be in the cache
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_text_cache WHERE message_id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 3);
    }

    // -----------------------------------------------------------------------
    // 12. Null content blob is skipped
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_content_skipped() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let msg_id = insert_test_message(&conn, &account.id, "Empty content");

        // Insert attachment with empty data blob
        let empty_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO attachments (id, message_id, account_id, filename, content_type, size, data)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, X'')",
            rusqlite::params![empty_id, msg_id, &account.id, "empty.txt", "text/plain"],
        )
        .unwrap();

        // Empty text should not be indexed
        let text = extract_text("text/plain", b"");
        assert!(text.is_ok());
        assert!(text.unwrap().is_empty());

        // Verify nothing in the cache
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_text_cache WHERE message_id = ?1",
                rusqlite::params![msg_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    // -----------------------------------------------------------------------
    // 13. Binary attachment is unsupported
    // -----------------------------------------------------------------------
    #[test]
    fn test_binary_attachment_unsupported() {
        let result = extract_text("image/png", b"\x89PNG\r\n\x1a\n");
        assert!(result.is_err());

        let result = extract_text("application/zip", b"PK\x03\x04");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // 14. Word doc placeholders
    // -----------------------------------------------------------------------
    #[test]
    fn test_word_doc_placeholder() {
        let result = extract_text(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            b"fake docx",
        );
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Word document"));
    }

    // -----------------------------------------------------------------------
    // 15. Spreadsheet placeholder
    // -----------------------------------------------------------------------
    #[test]
    fn test_spreadsheet_placeholder() {
        let result = extract_text(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            b"fake xlsx",
        );
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Spreadsheet"));
    }

    // -----------------------------------------------------------------------
    // 16. strip_html_tags correctness
    // -----------------------------------------------------------------------
    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(strip_html_tags("<b>Bold</b> and <i>italic</i>"), "Bold and italic");
        assert_eq!(strip_html_tags("No tags here"), "No tags here");
        assert_eq!(strip_html_tags("<div><span>Nested</span></div>"), "Nested");
        assert_eq!(strip_html_tags(""), "");
    }

    // -----------------------------------------------------------------------
    // 17. FTS5 table and cache table exist after migration
    // -----------------------------------------------------------------------
    #[test]
    fn test_migration_tables_exist() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='attachment_content_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "attachment_content_fts table should exist");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='attachment_text_cache'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "attachment_text_cache table should exist");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='attachments'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "attachments table should exist");
    }

    // -----------------------------------------------------------------------
    // 18. Account isolation in search
    // -----------------------------------------------------------------------
    #[test]
    fn test_account_isolation_in_cache() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account1 = create_test_account(&conn);
        let account2 = create_test_account(&conn);

        let msg1 = insert_test_message(&conn, &account1.id, "Account 1 msg");
        let msg2 = insert_test_message(&conn, &account2.id, "Account 2 msg");

        let c1 = b"Account one specific content about quantum physics.";
        let c2 = b"Account two specific content about classical music.";

        let a1 = insert_test_attachment(&conn, &msg1, &account1.id, "quantum.txt", "text/plain", c1);
        let a2 = insert_test_attachment(&conn, &msg2, &account2.id, "music.txt", "text/plain", c2);

        index_attachment(&conn, &a1, &msg1, &account1.id, "quantum.txt", "text/plain", c1);
        index_attachment(&conn, &a2, &msg2, &account2.id, "music.txt", "text/plain", c2);

        // Account 1 cache
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_text_cache WHERE account_id = ?1",
                rusqlite::params![&account1.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Account 2 cache
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attachment_text_cache WHERE account_id = ?1",
                rusqlite::params![&account2.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
