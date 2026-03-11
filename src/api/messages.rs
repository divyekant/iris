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
pub struct UnsubscribeResponse {
    pub success: bool,
    pub method: String,
    pub url: Option<String>,
}

pub async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<UnsubscribeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Get the message's list_unsubscribe URL and post flag
    let (url, has_post): (String, bool) = conn
        .query_row(
            "SELECT list_unsubscribe, list_unsubscribe_post FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![id],
            |row| {
                let url: Option<String> = row.get(0)?;
                let post: bool = row.get::<_, bool>(1).unwrap_or(false);
                Ok((url.unwrap_or_default(), post))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if url.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    if url.starts_with("http") && has_post {
        // RFC 8058: Send POST request with List-Unsubscribe=One-Click
        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("List-Unsubscribe=One-Click")
            .send()
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;

        Ok(Json(UnsubscribeResponse {
            success: resp.status().is_success(),
            method: "one-click".to_string(),
            url: Some(url),
        }))
    } else if url.starts_with("http") {
        // Return the URL for the frontend to open in a new tab
        Ok(Json(UnsubscribeResponse {
            success: true,
            method: "url".to_string(),
            url: Some(url),
        }))
    } else if url.starts_with("mailto:") {
        // Return mailto URL for frontend to handle
        Ok(Json(UnsubscribeResponse {
            success: true,
            method: "mailto".to_string(),
            url: Some(url),
        }))
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

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

    // Validate folder against allowlist to prevent SQL injection
    const ALLOWED_FOLDERS: &[&str] = &["INBOX", "Sent", "Drafts", "Starred", "Archive", "Trash"];
    let raw_folder = params.folder.as_deref().unwrap_or("INBOX");
    let folder = if ALLOWED_FOLDERS.contains(&raw_folder) { raw_folder } else { "INBOX" };

    let limit = params.limit.unwrap_or(50).min(500);
    let offset = params.offset.unwrap_or(0).max(0);

    // Category filter matches ai_category column (case-insensitive)
    let category_filter = params.category.as_ref().map(|cat| cat.to_lowercase());

    // Virtual folder WHERE clauses — Starred/Trash/Drafts are flag-based, not folder-column based
    let folder_where = match folder {
        "Starred" => "m.is_starred = 1 AND m.is_deleted = 0",
        "Trash" => "m.is_deleted = 1",
        "Drafts" => "m.is_draft = 1 AND m.is_deleted = 0",
        // Safe: folder is validated against ALLOWED_FOLDERS above
        "INBOX" => "m.folder = 'INBOX' AND m.is_deleted = 0",
        "Sent" => "m.folder = 'Sent' AND m.is_deleted = 0",
        "Archive" => "m.folder = 'Archive' AND m.is_deleted = 0",
        _ => "m.folder = 'INBOX' AND m.is_deleted = 0",
    };

    let select_cols = "m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                       m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                       m.labels, m.ai_priority_label, m.ai_category";

    // Thread grouping: show only the latest message per thread using ROW_NUMBER()
    let (messages, unread, total) = if let Some(ref account_id) = params.account_id {
        // Single-account query
        let cat_clause = if category_filter.is_some() { " AND LOWER(m.ai_category) = ?4" } else { "" };

        let query = format!(
            "WITH threaded AS (
                SELECT m.*, ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(m.thread_id, m.id)
                    ORDER BY m.date DESC
                ) as rn
                FROM messages m
                WHERE m.account_id = ?1 AND {folder_where}{cat_clause}
            )
            SELECT {select_cols} FROM threaded m WHERE m.rn = 1
            ORDER BY m.date DESC LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = conn.prepare(&query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let msgs: Vec<MessageSummary> = if let Some(ref cat) = category_filter {
            stmt.query_map(rusqlite::params![account_id, limit, offset, cat], MessageSummary::from_row)
        } else {
            stmt.query_map(rusqlite::params![account_id, limit, offset], MessageSummary::from_row)
        }
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let count_cat = if category_filter.is_some() { " AND LOWER(m.ai_category) = ?2" } else { "" };
        let unread: i64 = if let Some(ref cat) = category_filter {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m WHERE m.account_id = ?1 AND {folder_where} AND m.is_read = 0{count_cat}"),
                rusqlite::params![account_id, cat],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m WHERE m.account_id = ?1 AND {folder_where} AND m.is_read = 0"),
                rusqlite::params![account_id],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        let total: i64 = if let Some(ref cat) = category_filter {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m WHERE m.account_id = ?1 AND {folder_where}{count_cat}"),
                rusqlite::params![account_id, cat],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m WHERE m.account_id = ?1 AND {folder_where}"),
                rusqlite::params![account_id],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        (msgs, unread, total)
    } else {
        // Unified inbox: messages from all active accounts, merged by date DESC
        let unified_where = format!("a.is_active = 1 AND {folder_where}");
        let cat_clause = if category_filter.is_some() { " AND LOWER(m.ai_category) = ?3" } else { "" };

        let query = format!(
            "WITH threaded AS (
                SELECT m.*, ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(m.thread_id, m.id)
                    ORDER BY m.date DESC
                ) as rn
                FROM messages m
                JOIN accounts a ON m.account_id = a.id
                WHERE {unified_where}{cat_clause}
            )
            SELECT {select_cols} FROM threaded m WHERE m.rn = 1
            ORDER BY m.date DESC LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn.prepare(&query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let msgs: Vec<MessageSummary> = if let Some(ref cat) = category_filter {
            stmt.query_map(rusqlite::params![limit, offset, cat], MessageSummary::from_row)
        } else {
            stmt.query_map(rusqlite::params![limit, offset], MessageSummary::from_row)
        }
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let count_cat = if category_filter.is_some() { " AND LOWER(m.ai_category) = ?1" } else { "" };
        let unread: i64 = if let Some(ref cat) = category_filter {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m JOIN accounts a ON m.account_id = a.id WHERE {unified_where} AND m.is_read = 0{count_cat}"),
                rusqlite::params![cat],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m JOIN accounts a ON m.account_id = a.id WHERE {unified_where} AND m.is_read = 0"),
                [],
                |row| row.get(0),
            ).unwrap_or(0)
        };

        let total: i64 = if let Some(ref cat) = category_filter {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m JOIN accounts a ON m.account_id = a.id WHERE {unified_where}{count_cat}"),
                rusqlite::params![cat],
                |row| row.get(0),
            ).unwrap_or(0)
        } else {
            conn.query_row(
                &format!("SELECT COUNT(DISTINCT COALESCE(m.thread_id, m.id)) FROM messages m JOIN accounts a ON m.account_id = a.id WHERE {unified_where}"),
                [],
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
