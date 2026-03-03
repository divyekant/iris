use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::message::{self, MessageDetail, MessageSummary};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ListMessagesParams {
    pub account_id: Option<String>,
    pub folder: Option<String>,
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

    let (messages, unread, total) = if let Some(ref account_id) = params.account_id {
        // Single-account query
        let msgs = MessageSummary::list_by_folder(&conn, account_id, folder, limit, offset);

        let unread = message::unread_count(&conn, account_id, folder);

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND folder = ?2 AND is_deleted = 0",
                rusqlite::params![account_id, folder],
                |row| row.get(0),
            )
            .unwrap_or(0);

        (msgs, unread, total)
    } else {
        // Unified inbox: messages from all active accounts, merged by date DESC
        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.account_id, m.thread_id, m.folder, m.from_address, m.from_name,
                        m.subject, m.snippet, m.date, m.is_read, m.is_starred, m.has_attachments,
                        m.labels, m.ai_priority_label, m.ai_category
                 FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0
                 ORDER BY m.date DESC
                 LIMIT ?2 OFFSET ?3",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let msgs: Vec<MessageSummary> = stmt
            .query_map(rusqlite::params![folder, limit, offset], MessageSummary::from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .filter_map(|r| r.ok())
            .collect();

        let unread: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_read = 0 AND m.is_deleted = 0",
                rusqlite::params![folder],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages m
                 JOIN accounts a ON m.account_id = a.id
                 WHERE a.is_active = 1 AND m.folder = ?1 AND m.is_deleted = 0",
                rusqlite::params![folder],
                |row| row.get(0),
            )
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
) -> Result<Json<MessageDetail>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    MessageDetail::get_by_id(&conn, &id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
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
