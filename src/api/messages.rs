use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::trust;
use crate::models::message::{self, MessageDetail, MessageSummary};
use crate::AppState;

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
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let category_filter = if let Some(ref cat) = params.category {
        format!(" AND m.labels LIKE '%\"{}%'", cat.replace('\'', "''"))
    } else {
        String::new()
    };

    let (messages, unread, total) = if let Some(ref account_id) = params.account_id {
        // Single-account query
        let query = format!(
            "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                    m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                    m.labels, m.ai_priority_label, m.ai_category
             FROM messages m
             WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0{}
             ORDER BY m.date DESC
             LIMIT ?3 OFFSET ?4",
            category_filter
        );
        let mut stmt = conn
            .prepare(&query)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let msgs: Vec<MessageSummary> = stmt
            .query_map(rusqlite::params![account_id, folder, limit, offset], MessageSummary::from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let unread_query = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_read = 0 AND m.is_deleted = 0{}",
            category_filter
        );
        let unread: i64 = conn
            .query_row(&unread_query, rusqlite::params![account_id, folder], |row| row.get(0))
            .unwrap_or(0);

        let total_query = format!(
            "SELECT COUNT(*) FROM messages m WHERE m.account_id = ?1 AND m.folder = ?2 AND m.is_deleted = 0{}",
            category_filter
        );
        let total: i64 = conn
            .query_row(&total_query, rusqlite::params![account_id, folder], |row| row.get(0))
            .unwrap_or(0);

        (msgs, unread, total)
    } else {
        // Unified inbox: messages from all active accounts, merged by date DESC
        let query = format!(
            "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                    m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                    m.labels, m.ai_priority_label, m.ai_category
             FROM messages m
             JOIN accounts a ON m.account_id = a.id
             WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0{}
             ORDER BY m.date DESC
             LIMIT ?2 OFFSET ?3",
            category_filter
        );
        let mut stmt = conn
            .prepare(&query)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let msgs: Vec<MessageSummary> = stmt
            .query_map(rusqlite::params![folder, limit, offset], MessageSummary::from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let unread_query = format!(
            "SELECT COUNT(*) FROM messages m
             JOIN accounts a ON m.account_id = a.id
             WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_read = 0 AND m.is_deleted = 0{}",
            category_filter
        );
        let unread: i64 = conn
            .query_row(&unread_query, rusqlite::params![folder], |row| row.get(0))
            .unwrap_or(0);

        let total_query = format!(
            "SELECT COUNT(*) FROM messages m
             JOIN accounts a ON m.account_id = a.id
             WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0{}",
            category_filter
        );
        let total: i64 = conn
            .query_row(&total_query, rusqlite::params![folder], |row| row.get(0))
            .unwrap_or(0);

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
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let valid_actions = ["archive", "delete", "mark_read", "mark_unread", "star", "unstar"];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id_refs: Vec<&str> = req.ids.iter().map(|s| s.as_str()).collect();
    let updated = message::batch_update(&conn, &id_refs, &req.action);

    Ok(Json(BatchUpdateResponse { updated }))
}
