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
    pub blocked_senders: Vec<String>,
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

    // Optionally block ALL senders from the batch
    let blocked_senders = if req.block_sender.unwrap_or(false) {
        let mut senders = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for id in &req.ids {
            let sender: Option<String> = conn
                .query_row(
                    "SELECT from_address FROM messages WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            if let Some(email) = sender {
                let normalized = email.trim().to_lowercase();
                if seen.insert(normalized.clone()) {
                    BlockedSender::block(&conn, &email, Some("Reported as spam"));
                    senders.push(normalized);
                }
            }
        }

        senders
    } else {
        Vec::new()
    };

    Ok(Json(ReportSpamResponse {
        updated,
        blocked_senders,
    }))
}
