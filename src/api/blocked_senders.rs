use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::blocked_sender::BlockedSender;
use crate::models::message;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct BlockSenderRequest {
    pub email_address: String,
    pub reason: Option<String>,
}

/// GET /api/blocked-senders — list all blocked senders
pub async fn list_blocked_senders(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BlockedSender>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(BlockedSender::list(&conn)))
}

/// POST /api/blocked-senders — block a sender
pub async fn block_sender(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BlockSenderRequest>,
) -> Result<Json<BlockedSender>, StatusCode> {
    if req.email_address.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let blocked = BlockedSender::block(&conn, &req.email_address, req.reason.as_deref());
    Ok(Json(blocked))
}

/// DELETE /api/blocked-senders/:id — unblock a sender
pub async fn unblock_sender(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if BlockedSender::unblock(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Debug, Deserialize)]
pub struct ReportSpamRequest {
    pub ids: Vec<String>,
    pub block_sender: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ReportSpamResponse {
    pub updated: usize,
    pub blocked_sender: Option<String>,
}

/// POST /api/messages/report-spam — report messages as spam and optionally block sender
pub async fn report_spam(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReportSpamRequest>,
) -> Result<Json<ReportSpamResponse>, StatusCode> {
    if req.ids.is_empty() || req.ids.len() > 1000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Move messages to Spam folder
    let id_refs: Vec<&str> = req.ids.iter().map(|s| s.as_str()).collect();
    let updated = message::batch_update(&conn, &id_refs, "spam");

    // Optionally block the sender — look up from_address of the first message
    let blocked_sender = if req.block_sender.unwrap_or(false) {
        // Get the sender's email from the first message
        let sender: Option<String> = req.ids.first().and_then(|id| {
            conn.query_row(
                "SELECT from_address FROM messages WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .ok()
            .flatten()
        });

        if let Some(ref email) = sender {
            BlockedSender::block(&conn, email, Some("Reported as spam"));
            sender
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(ReportSpamResponse {
        updated,
        blocked_sender,
    }))
}
