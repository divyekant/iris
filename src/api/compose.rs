use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::refresh::ensure_fresh_token;
use crate::models::account::Account;
use crate::models::message::{self, InsertMessage, MessageDetail, MessageSummary};
use crate::smtp::{self, ComposeRequest};
use crate::AppState;

// ---------------------------------------------------------------------------
// POST /api/send — send an email
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SendResponse {
    pub message_id: String,
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
        message_id: id,
    }))
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

// ---------------------------------------------------------------------------
// POST /api/messages/{id}/redirect — bounce/redirect an email
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RedirectRequest {
    pub to: String,
}

#[derive(Serialize)]
pub struct RedirectResponse {
    pub redirected: bool,
    pub to: String,
}

/// Validates that a string looks like a valid email address.
fn is_valid_email(email: &str) -> bool {
    let trimmed = email.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Must contain exactly one @, with text on both sides, and a dot in the domain
    let parts: Vec<&str> = trimmed.splitn(2, '@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0], parts[1]);
    !local.is_empty() && !domain.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

pub async fn redirect_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<RedirectRequest>,
) -> Result<Json<RedirectResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Validate email
    let to = req.to.trim().to_string();
    if !is_valid_email(&to) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid recipient email address"})),
        ));
    }

    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    // Look up the original message
    let original = MessageDetail::get_by_id(&conn, &id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "message not found"})))
    })?;

    // Get the account for SMTP credentials
    let account = Account::get_by_id(&conn, &original.account_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "account not found"})))
    })?;

    if !account.is_active {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "account is inactive"})),
        ));
    }

    // Refresh OAuth token if needed
    let access_token = ensure_fresh_token(&state.db, &account, &state.config)
        .await
        .map_err(|e| {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": format!("token refresh: {e}")})))
        })?;

    // Build the redirected email with Resent-* headers
    let email = smtp::build_redirect_email(&account.email, &to, &original)
        .map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    // Send via SMTP
    smtp::send_email(&account, access_token.as_deref(), email)
        .await
        .map_err(|e| {
            (StatusCode::BAD_GATEWAY, Json(serde_json::json!({"error": e.to_string()})))
        })?;

    tracing::info!(
        account = %account.email,
        message_id = %id,
        to = %to,
        "Email redirected"
    );

    Ok(Json(RedirectResponse {
        redirected: true,
        to,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_email_accepts_valid() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("name+tag@sub.domain.org"));
        assert!(is_valid_email("  alice@test.co  ")); // trimmed
    }

    #[test]
    fn test_is_valid_email_rejects_invalid() {
        assert!(!is_valid_email(""));
        assert!(!is_valid_email("   "));
        assert!(!is_valid_email("noatsign"));
        assert!(!is_valid_email("@nodomain"));
        assert!(!is_valid_email("nolocal@"));
        assert!(!is_valid_email("user@nodot"));
        assert!(!is_valid_email("user@.dot"));
        assert!(!is_valid_email("user@dot."));
    }

    #[test]
    fn test_redirect_request_deserialization() {
        let json = r#"{"to": "alice@example.com"}"#;
        let req: RedirectRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.to, "alice@example.com");
    }

    #[test]
    fn test_redirect_response_serialization() {
        let resp = RedirectResponse {
            redirected: true,
            to: "bob@test.com".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"redirected\":true"));
        assert!(json.contains("\"to\":\"bob@test.com\""));
    }

    #[test]
    fn test_message_lookup_for_redirect() {
        use crate::db::create_test_pool;
        use crate::models::account::{Account, CreateAccount};
        use crate::models::message::InsertMessage;

        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account = Account::create(&conn, &CreateAccount {
            provider: "gmail".to_string(),
            email: "redirect-test@example.com".to_string(),
            display_name: Some("Redirect Test".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("redirect-test@example.com".to_string()),
            password: None,
        });

        let msg = InsertMessage {
            account_id: account.id.clone(),
            message_id: Some("<redirect-orig@example.com>".to_string()),
            thread_id: None,
            folder: "INBOX".to_string(),
            from_address: Some("sender@example.com".to_string()),
            from_name: Some("Original Sender".to_string()),
            to_addresses: Some(r#"["redirect-test@example.com"]"#.to_string()),
            cc_addresses: None,
            bcc_addresses: None,
            subject: Some("Important message".to_string()),
            date: Some(1700000000),
            snippet: Some("Preview...".to_string()),
            body_text: Some("Full body of the original email".to_string()),
            body_html: Some("<p>Full body of the original email</p>".to_string()),
            is_read: true,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: Some(1),
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(2048),
        };

        let id = InsertMessage::insert(&conn, &msg).unwrap();

        // Verify message can be looked up
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert_eq!(detail.from_address.as_deref(), Some("sender@example.com"));
        assert_eq!(detail.subject.as_deref(), Some("Important message"));
        assert_eq!(detail.account_id, account.id);

        // Verify non-existent message returns None
        assert!(MessageDetail::get_by_id(&conn, "nonexistent-id").is_none());
    }
}
