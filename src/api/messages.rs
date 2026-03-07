use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::trust;
use crate::models::message::{self, MessageDetail, MessageSummary};
use crate::AppState;
extern crate mailparse;

#[derive(Debug, Serialize)]
pub struct MessageDetailResponse {
    #[serde(flatten)]
    pub message: MessageDetail,
    pub trust: trust::TrustIndicators,
    pub tracking_pixels: Vec<trust::TrackingPixel>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesParams {
    pub account_id: Option<String>,
    pub folder: Option<String>,
    pub category: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ListMessagesResponse {
    pub messages: Vec<MessageSummary>,
    pub unread_count: i64,
    pub total: i64,
}

pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListMessagesParams>,
) -> Result<Json<ListMessagesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let folder = params.folder.as_deref().unwrap_or("INBOX");
    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Build category filter as a bound parameter (not string-interpolated) to prevent SQL injection
    // Escape LIKE wildcards in user input to prevent unintended matches
    let category_like = params.category.as_ref().map(|cat| {
        let escaped = cat.replace('%', "\\%").replace('_', "\\_");
        format!("%\"{}%", escaped)
    });

    let (messages, unread, total) = if let Some(ref account_id) = params.account_id {
        // Single-account query
        let (query, has_cat) = if category_like.is_some() {
            (
                "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                        m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                        m.labels, m.ai_priority_label, m.ai_category
                 FROM messages m
                 WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0 AND m.labels LIKE ?5
                 ORDER BY m.date DESC
                 LIMIT ?3 OFFSET ?4".to_string(),
                true,
            )
        } else {
            (
                "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                        m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                        m.labels, m.ai_priority_label, m.ai_category
                 FROM messages m
                 WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0
                 ORDER BY m.date DESC
                 LIMIT ?3 OFFSET ?4".to_string(),
                false,
            )
        };
        let mut stmt = conn
            .prepare(&query)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let msgs: Vec<MessageSummary> = if has_cat {
            stmt.query_map(rusqlite::params![account_id, folder, limit, offset, &category_like], MessageSummary::from_row)
        } else {
            stmt.query_map(rusqlite::params![account_id, folder, limit, offset], MessageSummary::from_row)
        }
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let unread: i64 = if has_cat {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_read = 0 AND m.is_deleted = 0 AND m.labels LIKE ?3",
                rusqlite::params![account_id, folder, &category_like],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_read = 0 AND m.is_deleted = 0",
                rusqlite::params![account_id, folder],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        let total: i64 = if has_cat {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0 AND m.labels LIKE ?3",
                rusqlite::params![account_id, folder, &category_like],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0",
                rusqlite::params![account_id, folder],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        (msgs, unread, total)
    } else {
        // Unified inbox: messages from all active accounts, merged by date DESC
        let (query, has_cat) = if category_like.is_some() {
            (
                "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                        m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                        m.labels, m.ai_priority_label, m.ai_category
                 FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0 AND m.labels LIKE ?4
                 ORDER BY m.date DESC
                 LIMIT ?2 OFFSET ?3".to_string(),
                true,
            )
        } else {
            (
                "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                        m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                        m.labels, m.ai_priority_label, m.ai_category
                 FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0
                 ORDER BY m.date DESC
                 LIMIT ?2 OFFSET ?3".to_string(),
                false,
            )
        };
        let mut stmt = conn
            .prepare(&query)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let msgs: Vec<MessageSummary> = if has_cat {
            stmt.query_map(rusqlite::params![folder, limit, offset, &category_like], MessageSummary::from_row)
        } else {
            stmt.query_map(rusqlite::params![folder, limit, offset], MessageSummary::from_row)
        }
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let unread: i64 = if has_cat {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_read = 0 AND m.is_deleted = 0 AND m.labels LIKE ?2",
                rusqlite::params![folder, &category_like],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_read = 0 AND m.is_deleted = 0",
                rusqlite::params![folder],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        let total: i64 = if has_cat {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0 AND m.labels LIKE ?2",
                rusqlite::params![folder, &category_like],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0",
                rusqlite::params![folder],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        (msgs, unread, total)
    };

    Ok(Json(ListMessagesResponse {
        messages,
        unread_count: unread,
        total,
    }))
}

pub async fn get_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<MessageDetailResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let message = MessageDetail::get_by_id(&conn, &id).ok_or(StatusCode::NOT_FOUND)?;

    // Query raw_headers separately (not part of MessageDetail)
    let raw_headers: Option<String> = conn
        .query_row(
            "SELECT raw_headers FROM messages WHERE id = ?1",
            rusqlite::params![&id],
            |row| row.get(0),
        )
        .ok();

    let trust_indicators =
        trust::extract_trust_indicators(raw_headers.as_deref().unwrap_or(""));
    let tracking_pixels =
        trust::detect_tracking_pixels(message.body_html.as_deref().unwrap_or(""));

    Ok(Json(MessageDetailResponse {
        message,
        trust: trust_indicators,
        tracking_pixels,
    }))
}

pub async fn mark_message_read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if message::mark_as_read(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Debug, Deserialize)]
pub struct BatchUpdateRequest {
    pub ids: Vec<String>,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct BatchUpdateResponse {
    pub updated: usize,
}

pub async fn batch_update_messages(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchUpdateRequest>,
) -> Result<Json<BatchUpdateResponse>, StatusCode> {
    // Cap batch size to prevent DoS via unbounded IN clause
    if req.ids.is_empty() || req.ids.len() > 1000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let valid_actions = ["archive", "delete", "mark_read", "mark_unread", "star", "unstar"];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id_refs: Vec<&str> = req.ids.iter().map(|s| s.as_str()).collect();
    let updated = message::batch_update(&conn, &id_refs, &req.action);

    Ok(Json(BatchUpdateResponse { updated }))
}

#[derive(Debug, Serialize)]
pub struct FixEncodingResponse {
    pub fixed: usize,
}

/// Re-decode RFC 2047 encoded subjects and from_names from raw_headers.
/// Fixes messages that were synced before the decode_rfc2047 fix was applied.
pub async fn fix_encoding(
    State(state): State<Arc<AppState>>,
) -> Result<Json<FixEncodingResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find messages with encoded subjects or from_names
    let mut stmt = conn
        .prepare(
            "SELECT id, subject, from_name, raw_headers FROM messages \
             WHERE raw_headers IS NOT NULL AND (subject LIKE '%=?%' OR from_name LIKE '%=?%')",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows: Vec<(String, Option<String>, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut fixed = 0usize;
    for (id, _old_subject, _old_from_name, raw_headers) in &rows {
        let headers = match mailparse::parse_headers(raw_headers.as_bytes()) {
            Ok((hdrs, _)) => hdrs,
            Err(_) => continue,
        };

        let new_subject = headers.iter().find(|h| h.get_key_ref() == "Subject").map(|h| h.get_value());
        let new_from_name = headers
            .iter()
            .find(|h| h.get_key_ref() == "From")
            .and_then(|h| {
                let val = h.get_value();
                // Extract display name from "Name <email>" format
                if let Some(pos) = val.rfind('<') {
                    let name = val[..pos].trim().trim_matches('"').to_string();
                    if name.is_empty() { None } else { Some(name) }
                } else {
                    None
                }
            });

        let updated = conn
            .execute(
                "UPDATE messages SET subject = COALESCE(?1, subject), from_name = COALESCE(?2, from_name) WHERE id = ?3",
                rusqlite::params![new_subject, new_from_name, id],
            )
            .unwrap_or(0);

        if updated > 0 {
            fixed += 1;
        }
    }

    // Also update the FTS5 index for fixed messages
    if fixed > 0 {
        let _ = conn.execute_batch(
            "INSERT INTO messages_fts(messages_fts) VALUES('rebuild')"
        );
    }

    Ok(Json(FixEncodingResponse { fixed }))
}
