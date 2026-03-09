use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::account::Account;
use crate::models::message::{self, MessageSummary};
use crate::smtp::ComposeRequest;
use crate::AppState;

// ---------------------------------------------------------------------------
// POST /api/send — queue an email for sending (with undo delay)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SendResponse {
    pub id: String,
    pub send_at: i64,
    pub can_undo: bool,
}

pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ComposeRequest>,
) -> Result<Json<SendResponse>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    // Validate the account exists and is active
    let account = Account::get_by_id(&conn, &req.account_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "account not found"})))
    })?;

    if !account.is_active {
        return Err((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "account is inactive"}))));
    }

    // Read undo-send delay from config
    let delay_seconds: i64 = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'undo_send_delay_seconds'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);

    let now = chrono::Utc::now().timestamp();
    let send_at = now + delay_seconds;
    let id = uuid::Uuid::new_v4().to_string();

    let to_json = serde_json::to_string(&req.to).unwrap_or_else(|_| "[]".to_string());
    let cc_json = if req.cc.is_empty() { None } else { serde_json::to_string(&req.cc).ok() };
    let bcc_json = if req.bcc.is_empty() { None } else { serde_json::to_string(&req.bcc).ok() };

    conn.execute(
        "INSERT INTO pending_sends (id, account_id, to_addresses, cc_addresses, bcc_addresses, subject, body_text, body_html, in_reply_to, references_header, send_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            id,
            req.account_id,
            to_json,
            cc_json,
            bcc_json,
            req.subject,
            req.body_text,
            req.body_html,
            req.in_reply_to,
            req.references,
            send_at,
        ],
    )
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("failed to queue send: {e}")})))
    })?;

    tracing::info!(
        account = %account.email,
        to = ?req.to,
        subject = %req.subject,
        send_at = send_at,
        delay = delay_seconds,
        "Email queued for sending (undo available)"
    );

    Ok(Json(SendResponse {
        id,
        send_at,
        can_undo: true,
    }))
}

// ---------------------------------------------------------------------------
// POST /api/send/cancel/:id — cancel a pending send
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CancelResponse {
    pub cancelled: bool,
}

pub async fn cancel_send(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<CancelResponse>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    let updated = conn
        .execute(
            "UPDATE pending_sends SET status = 'cancelled' WHERE id = ?1 AND status = 'pending'",
            rusqlite::params![id],
        )
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("cancel failed: {e}")})))
        })?;

    if updated > 0 {
        tracing::info!(pending_send_id = %id, "Pending send cancelled");
        Ok(Json(CancelResponse { cancelled: true }))
    } else {
        // Check if it exists at all
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_sends WHERE id = ?1)",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !exists {
            Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "pending send not found"}))))
        } else {
            // Already sent or already cancelled
            Ok(Json(CancelResponse { cancelled: false }))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /api/config/undo-send-delay — read the delay
// PUT /api/config/undo-send-delay — update the delay (5-30)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct UndoSendDelayResponse {
    pub delay_seconds: i64,
}

#[derive(Deserialize)]
pub struct SetUndoSendDelayRequest {
    pub delay_seconds: i64,
}

pub async fn get_undo_send_delay(
    State(state): State<Arc<AppState>>,
) -> Result<Json<UndoSendDelayResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let delay: i64 = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'undo_send_delay_seconds'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "10".to_string())
        .parse()
        .unwrap_or(10);

    Ok(Json(UndoSendDelayResponse { delay_seconds: delay }))
}

pub async fn set_undo_send_delay(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SetUndoSendDelayRequest>,
) -> Result<Json<UndoSendDelayResponse>, (StatusCode, Json<serde_json::Value>)> {
    if input.delay_seconds < 5 || input.delay_seconds > 30 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "delay must be between 5 and 30 seconds"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    conn.execute(
        "INSERT INTO config (key, value) VALUES ('undo_send_delay_seconds', ?1)
         ON CONFLICT(key) DO UPDATE SET value = ?1",
        rusqlite::params![input.delay_seconds.to_string()],
    )
    .map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("save failed: {e}")})))
    })?;

    tracing::info!(delay = input.delay_seconds, "Undo send delay updated");

    Ok(Json(UndoSendDelayResponse {
        delay_seconds: input.delay_seconds,
    }))
}

// ---------------------------------------------------------------------------
// Internal: actually send a pending email (called by the job worker)
// ---------------------------------------------------------------------------

/// A row from the pending_sends table.
#[derive(Debug, Clone)]
pub struct PendingSend {
    pub id: String,
    pub account_id: String,
    pub to_addresses: String,
    pub cc_addresses: Option<String>,
    pub bcc_addresses: Option<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references_header: Option<String>,
}

/// Find all pending sends that are ready to be sent.
pub fn claim_pending_sends(conn: &rusqlite::Connection) -> Vec<PendingSend> {
    let now = chrono::Utc::now().timestamp();
    let mut stmt = match conn.prepare(
        "SELECT id, account_id, to_addresses, cc_addresses, bcc_addresses, subject, body_text, body_html, in_reply_to, references_header
         FROM pending_sends
         WHERE status = 'pending' AND send_at <= ?1",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    stmt.query_map(rusqlite::params![now], |row| {
        Ok(PendingSend {
            id: row.get(0)?,
            account_id: row.get(1)?,
            to_addresses: row.get(2)?,
            cc_addresses: row.get(3)?,
            bcc_addresses: row.get(4)?,
            subject: row.get(5)?,
            body_text: row.get(6)?,
            body_html: row.get(7)?,
            in_reply_to: row.get(8)?,
            references_header: row.get(9)?,
        })
    })
    .ok()
    .map(|rows| rows.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Mark a pending send as sent.
pub fn mark_pending_sent(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "UPDATE pending_sends SET status = 'sent' WHERE id = ?1",
        rusqlite::params![id],
    )
    .ok();
}

/// Mark a pending send as failed.
pub fn mark_pending_failed(conn: &rusqlite::Connection, id: &str) {
    conn.execute(
        "UPDATE pending_sends SET status = 'failed' WHERE id = ?1",
        rusqlite::params![id],
    )
    .ok();
}

// ---------------------------------------------------------------------------
// Draft endpoints (unchanged)
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
