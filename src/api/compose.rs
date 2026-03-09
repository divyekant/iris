use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::refresh::ensure_fresh_token;
use crate::models::account::Account;
use crate::models::message::{self, InsertMessage, MessageSummary};
use crate::smtp::{self, ComposeRequest};
use crate::AppState;

// ---------------------------------------------------------------------------
// POST /api/send — send an email (immediate or scheduled)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SendResponse {
    pub id: String,
    pub send_at: i64,
    pub scheduled: bool,
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ComposeRequest>,
) -> Result<Json<SendResponse>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    let account = Account::get_by_id(&conn, &req.account_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "account not found"})))
    })?;

    if !account.is_active {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "account is inactive"}))));
    }

    // If schedule_at is set and in the future (more than 5s from now), insert as pending
    let now = chrono::Utc::now().timestamp();
    if let Some(schedule_at) = req.schedule_at {
        if schedule_at > now + 5 {
            let to_json = serde_json::to_string(&req.to).unwrap_or_default();
            let cc_json = if req.cc.is_empty() { None } else { serde_json::to_string(&req.cc).ok() };
            let bcc_json = if req.bcc.is_empty() { None } else { serde_json::to_string(&req.bcc).ok() };

            let id: String = conn.query_row(
                "INSERT INTO pending_sends (account_id, to_addresses, cc_addresses, bcc_addresses, subject, body_text, body_html, in_reply_to, references_header, send_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 RETURNING id",
                rusqlite::params![
                    req.account_id,
                    to_json,
                    cc_json,
                    bcc_json,
                    req.subject,
                    req.body_text,
                    req.body_html,
                    req.in_reply_to,
                    req.references,
                    schedule_at,
                ],
                |row| row.get(0),
            ).map_err(|e| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("failed to schedule: {e}")})))
            })?;

            tracing::info!(account = %account.email, to = ?req.to, schedule_at, "Email scheduled");

            return Ok(Json(SendResponse {
                id,
                send_at: schedule_at,
                scheduled: true,
            }));
        }
    }

    // Immediate send path
    // Refresh OAuth token if needed
    let access_token = ensure_fresh_token(&state.db, &account, &state.config)
        .await
        .map_err(|e| {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": format!("token refresh: {e}")})))
        })?;

    // Build email
    let email = smtp::build_email(
        &account.email,
        account.display_name.as_deref(),
        &req,
    )
    .map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
    })?;

    // Extract the Message-ID that lettre generated
    let rfc_message_id = email
        .headers()
        .get_raw("Message-ID")
        .map(|v| v.to_string());

    // Send via SMTP
    smtp::send_email(&account, access_token.as_deref(), email)
        .await
        .map_err(|e| {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    // Store in Sent folder
    let to_json = serde_json::to_string(&req.to).ok();
    let cc_json = if req.cc.is_empty() { None } else { serde_json::to_string(&req.cc).ok() };
    let bcc_json = if req.bcc.is_empty() { None } else { serde_json::to_string(&req.bcc).ok() };

    let sent_msg = InsertMessage {
        account_id: req.account_id.clone(),
        message_id: rfc_message_id.clone(),
        thread_id: req.in_reply_to.as_ref().map(|r| r.trim_matches(|c| c == '<' || c == '>').to_string()),
        folder: "Sent".to_string(),
        from_address: Some(account.email.clone()),
        from_name: account.display_name.clone(),
        to_addresses: to_json,
        cc_addresses: cc_json,
        bcc_addresses: bcc_json,
        subject: Some(req.subject.clone()),
        date: Some(chrono::Utc::now().timestamp()),
        snippet: Some(req.body_text.chars().take(200).collect()),
        body_text: Some(req.body_text.clone()),
        body_html: req.body_html.clone(),
        is_read: true,
        is_starred: false,
        is_draft: false,
        labels: None,
        uid: None,
        modseq: None,
        raw_headers: None,
        has_attachments: false,
        attachment_names: None,
        size_bytes: None,
    };

    let id = InsertMessage::insert(&conn, &sent_msg).expect("sent message should always insert");

    tracing::info!(account = %account.email, to = ?req.to, subject = %req.subject, "Email sent");

    Ok(Json(SendResponse {
        id,
        send_at: now,
        scheduled: false,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/send/scheduled — list scheduled sends
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ScheduledSend {
    pub id: String,
    pub account_id: String,
    pub to_addresses: String,
    pub cc_addresses: Option<String>,
    pub bcc_addresses: Option<String>,
    pub subject: String,
    pub body_text: String,
    pub send_at: i64,
    pub created_at: i64,
    pub status: String,
}

pub async fn list_scheduled(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ScheduledSend>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = chrono::Utc::now().timestamp();

    // Only show sends scheduled more than 30s in the future (exclude undo-send items)
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, to_addresses, cc_addresses, bcc_addresses, subject, body_text, send_at, created_at, status
             FROM pending_sends
             WHERE status = 'pending' AND send_at > ?1
             ORDER BY send_at ASC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let threshold = now + 30;
    let items: Vec<ScheduledSend> = stmt
        .query_map(rusqlite::params![threshold], |row| {
            Ok(ScheduledSend {
                id: row.get(0)?,
                account_id: row.get(1)?,
                to_addresses: row.get(2)?,
                cc_addresses: row.get(3)?,
                bcc_addresses: row.get(4)?,
                subject: row.get(5)?,
                body_text: row.get(6)?,
                send_at: row.get(7)?,
                created_at: row.get(8)?,
                status: row.get(9)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(items))
}

// ---------------------------------------------------------------------------
// DELETE /api/send/scheduled/:id — cancel a scheduled send
// ---------------------------------------------------------------------------

pub async fn cancel_scheduled(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    let updated = conn
        .execute(
            "UPDATE pending_sends SET status = 'cancelled' WHERE id = ?1 AND status = 'pending'",
            rusqlite::params![id],
        )
        .map_err(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
        })?;

    if updated > 0 {
        tracing::info!(id = %id, "Scheduled send cancelled");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "scheduled send not found or already sent"}))))
    }
}

// ---------------------------------------------------------------------------
// Draft endpoints
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SaveDraftRequest {
    pub account_id: String,
    pub draft_id: Option<String>,
    pub to: Option<Vec<String>>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body_text: String,
    pub body_html: Option<String>,
}

#[derive(Serialize)]
pub struct DraftResponse {
    pub draft_id: String,
}

pub async fn save_draft(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveDraftRequest>,
) -> Result<Json<DraftResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let to_json = req.to.as_ref().and_then(|v| serde_json::to_string(v).ok());
    let cc_json = req.cc.as_ref().and_then(|v| serde_json::to_string(v).ok());
    let bcc_json = req.bcc.as_ref().and_then(|v| serde_json::to_string(v).ok());

    let draft_id = message::save_draft(
        &conn,
        req.draft_id.as_deref(),
        &req.account_id,
        to_json.as_deref(),
        cc_json.as_deref(),
        bcc_json.as_deref(),
        req.subject.as_deref(),
        &req.body_text,
        req.body_html.as_deref(),
    );

    Ok(Json(DraftResponse { draft_id }))
}

pub async fn list_drafts(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<MessageSummary>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account_id = params.get("account_id").map(|s| s.as_str()).unwrap_or("");
    let drafts = message::list_drafts(&conn, account_id);
    Ok(Json(drafts))
}

pub async fn delete_draft(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if message::delete_draft(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
