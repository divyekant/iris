use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use mailparse::MailHeaderMap;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use serde::Deserialize;
use std::sync::Arc;

use crate::api::compose::{save_draft, SaveDraftRequest, send_message, SendResponse};
use crate::api::unified_auth::{AuthContext, Permission};
use crate::models::account::Account;
use crate::models::message::MessageDetail;
use crate::smtp::ComposeRequest;
use crate::AppState;

type Conn = PooledConnection<SqliteConnectionManager>;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ReplyRequest {
    pub message_id: String,
    pub body: String,
    #[serde(default)]
    pub reply_all: bool,
}

#[derive(Deserialize)]
pub struct ForwardRequest {
    pub message_id: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub body: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type ApiError = (StatusCode, Json<serde_json::Value>);

fn api_err(status: StatusCode, msg: &str) -> ApiError {
    (status, Json(serde_json::json!({"error": msg})))
}

/// Look up the original message, enforcing account-scope for API keys.
fn resolve_original(
    conn: &Conn,
    message_id: &str,
    auth: &AuthContext,
) -> Result<MessageDetail, ApiError> {
    let original = MessageDetail::get_by_id(conn, message_id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "message not found"))?;

    // Account-scope check for API keys
    if let Some(scope) = auth.account_scope() {
        if original.account_id != scope {
            return Err(api_err(StatusCode::FORBIDDEN, "message outside account scope"));
        }
    }

    Ok(original)
}

/// Extract the References header value from raw_headers stored in the DB.
fn extract_references_header(conn: &Conn, msg_id: &str) -> Option<String> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw_headers FROM messages WHERE id = ?1",
            rusqlite::params![msg_id],
            |row| row.get(0),
        )
        .ok()?;

    let raw = raw?;
    let headers = mailparse::parse_headers(raw.as_bytes()).ok()?.0;
    headers.get_first_value("References")
}

/// Prepend a prefix (e.g. "Re: " or "Fwd: ") to a subject if not already present.
fn prepend_subject(subject: Option<&str>, prefix: &str) -> String {
    let subj = subject.unwrap_or("(no subject)");
    if subj.starts_with(prefix) {
        subj.to_string()
    } else {
        format!("{prefix}{subj}")
    }
}

/// Format a unix timestamp as a human-readable date string.
fn format_date(ts: Option<i64>) -> String {
    ts.and_then(|t| {
        chrono::DateTime::from_timestamp(t, 0)
            .map(|dt| dt.format("%a, %b %d, %Y at %H:%M").to_string())
    })
    .unwrap_or_else(|| "unknown date".to_string())
}

/// Build the quoted reply body.
fn build_reply_body(user_body: &str, original: &MessageDetail) -> String {
    let from = original
        .from_name
        .as_deref()
        .or(original.from_address.as_deref())
        .unwrap_or("unknown");
    let date = format_date(original.date);
    let original_text = original.body_text.as_deref().unwrap_or("");
    let quoted: String = original_text
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{user_body}\n\nOn {date}, {from} wrote:\n{quoted}")
}

/// Build the forwarded message body.
fn build_forward_body(user_body: &str, original: &MessageDetail) -> String {
    let from = original
        .from_address
        .as_deref()
        .unwrap_or("unknown");
    let date = format_date(original.date);
    let subject = original.subject.as_deref().unwrap_or("(no subject)");
    let original_text = original.body_text.as_deref().unwrap_or("");

    format!(
        "{user_body}\n\n---------- Forwarded message ----------\nFrom: {from}\nDate: {date}\nSubject: {subject}\n\n{original_text}"
    )
}

/// Build reply recipients. For reply-all, include original To + CC minus self.
fn build_reply_recipients(
    original: &MessageDetail,
    account_email: &str,
    reply_all: bool,
) -> (Vec<String>, Vec<String>) {
    let from = original
        .from_address
        .as_deref()
        .unwrap_or("")
        .to_string();

    if !reply_all {
        return (vec![from], vec![]);
    }

    let self_lower = account_email.to_lowercase();

    // Parse To addresses from the JSON array stored in to_addresses
    let mut to_addrs: Vec<String> = vec![from];
    if let Some(ref to_json) = original.to_addresses {
        if let Ok(addrs) = serde_json::from_str::<Vec<String>>(to_json) {
            for addr in addrs {
                if addr.to_lowercase() != self_lower && !to_addrs.contains(&addr) {
                    to_addrs.push(addr);
                }
            }
        }
    }

    // CC addresses go to CC, minus self
    let mut cc_addrs: Vec<String> = vec![];
    if let Some(ref cc_json) = original.cc_addresses {
        if let Ok(addrs) = serde_json::from_str::<Vec<String>>(cc_json) {
            for addr in addrs {
                if addr.to_lowercase() != self_lower
                    && !to_addrs.contains(&addr)
                    && !cc_addrs.contains(&addr)
                {
                    cc_addrs.push(addr);
                }
            }
        }
    }

    (to_addrs, cc_addrs)
}

/// Build References header for a reply: original's References + original's Message-ID.
fn build_references(existing_refs: Option<&str>, original_message_id: Option<&str>) -> Option<String> {
    match (existing_refs, original_message_id) {
        (Some(refs), Some(mid)) => Some(format!("{refs} {mid}")),
        (None, Some(mid)) => Some(mid.to_string()),
        (Some(refs), None) => Some(refs.to_string()),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// POST /api/reply
// ---------------------------------------------------------------------------

pub async fn reply(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReplyRequest>,
) -> Result<Json<SendResponse>, ApiError> {
    auth.require(Permission::SendWithApproval)
        .map_err(|s| (s, Json(serde_json::json!({"error": "insufficient permission"}))))?;

    let conn = state.db.get().map_err(|_| api_err(StatusCode::INTERNAL_SERVER_ERROR, "database error"))?;
    let original = resolve_original(&conn, &req.message_id, &auth)?;

    let account = Account::get_by_id(&conn, &original.account_id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "account not found"))?;

    let (to, cc) = build_reply_recipients(&original, &account.email, req.reply_all);
    let subject = prepend_subject(original.subject.as_deref(), "Re: ");
    let body_text = build_reply_body(&req.body, &original);

    let existing_refs = extract_references_header(&conn, &req.message_id);
    let references = build_references(
        existing_refs.as_deref(),
        original.message_id.as_deref(),
    );

    let compose = ComposeRequest {
        account_id: original.account_id.clone(),
        to,
        cc,
        bcc: vec![],
        subject,
        body_text,
        body_html: None,
        in_reply_to: original.message_id.clone(),
        references,
        attachments: vec![],
        schedule_at: None,
    };

    // Drop the connection before calling send_message (it needs its own)
    drop(conn);

    send_message(Extension(auth), State(state), Json(compose)).await
}

// ---------------------------------------------------------------------------
// POST /api/forward
// ---------------------------------------------------------------------------

pub async fn forward(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForwardRequest>,
) -> Result<Json<SendResponse>, ApiError> {
    auth.require(Permission::SendWithApproval)
        .map_err(|s| (s, Json(serde_json::json!({"error": "insufficient permission"}))))?;

    let conn = state.db.get().map_err(|_| api_err(StatusCode::INTERNAL_SERVER_ERROR, "database error"))?;
    let original = resolve_original(&conn, &req.message_id, &auth)?;

    let subject = prepend_subject(original.subject.as_deref(), "Fwd: ");
    let body_text = build_forward_body(&req.body, &original);

    let compose = ComposeRequest {
        account_id: original.account_id.clone(),
        to: req.to,
        cc: vec![],
        bcc: vec![],
        subject,
        body_text,
        body_html: None,
        in_reply_to: None,
        references: None,
        attachments: vec![],
        schedule_at: None,
    };

    drop(conn);

    send_message(Extension(auth), State(state), Json(compose)).await
}

// ---------------------------------------------------------------------------
// POST /api/drafts/reply
// ---------------------------------------------------------------------------

pub async fn draft_reply(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ReplyRequest>,
) -> Result<Json<crate::api::compose::DraftResponse>, StatusCode> {
    auth.require(Permission::DraftOnly)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let original = resolve_original(&conn, &req.message_id, &auth)
        .map_err(|e| e.0)?;

    let account = Account::get_by_id(&conn, &original.account_id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let (to, cc) = build_reply_recipients(&original, &account.email, req.reply_all);
    let subject = prepend_subject(original.subject.as_deref(), "Re: ");
    let body_text = build_reply_body(&req.body, &original);

    let draft_req = SaveDraftRequest {
        account_id: original.account_id.clone(),
        draft_id: None,
        to: Some(to),
        cc: if cc.is_empty() { None } else { Some(cc) },
        bcc: None,
        subject: Some(subject),
        body_text,
        body_html: None,
    };

    drop(conn);

    save_draft(Extension(auth), State(state), Json(draft_req)).await
}

// ---------------------------------------------------------------------------
// POST /api/drafts/forward
// ---------------------------------------------------------------------------

pub async fn draft_forward(
    Extension(auth): Extension<AuthContext>,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForwardRequest>,
) -> Result<Json<crate::api::compose::DraftResponse>, StatusCode> {
    auth.require(Permission::DraftOnly)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let original = resolve_original(&conn, &req.message_id, &auth)
        .map_err(|e| e.0)?;

    let subject = prepend_subject(original.subject.as_deref(), "Fwd: ");
    let body_text = build_forward_body(&req.body, &original);

    let draft_req = SaveDraftRequest {
        account_id: original.account_id.clone(),
        draft_id: None,
        to: Some(req.to),
        cc: None,
        bcc: None,
        subject: Some(subject),
        body_text,
        body_html: None,
    };

    drop(conn);

    save_draft(Extension(auth), State(state), Json(draft_req)).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::message::MessageDetail;

    fn make_test_message() -> MessageDetail {
        MessageDetail {
            id: "msg-1".into(),
            message_id: Some("<orig-123@example.com>".into()),
            account_id: "acct-1".into(),
            thread_id: None,
            folder: "INBOX".into(),
            from_address: Some("sender@example.com".into()),
            from_name: Some("Alice Sender".into()),
            to_addresses: Some(r#"["me@example.com","bob@example.com"]"#.into()),
            cc_addresses: Some(r#"["carol@example.com"]"#.into()),
            subject: Some("Original subject".into()),
            snippet: Some("Preview...".into()),
            date: Some(1700000000),
            body_text: Some("Hello,\nThis is the original message.\nBest regards.".into()),
            body_html: None,
            is_read: true,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
            ai_sentiment: None,
            ai_needs_reply: false,
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    #[test]
    fn test_prepend_subject_re() {
        assert_eq!(prepend_subject(Some("Hello"), "Re: "), "Re: Hello");
        assert_eq!(prepend_subject(Some("Re: Hello"), "Re: "), "Re: Hello");
        assert_eq!(prepend_subject(None, "Re: "), "Re: (no subject)");
    }

    #[test]
    fn test_prepend_subject_fwd() {
        assert_eq!(prepend_subject(Some("Hello"), "Fwd: "), "Fwd: Hello");
        assert_eq!(prepend_subject(Some("Fwd: Hello"), "Fwd: "), "Fwd: Hello");
    }

    #[test]
    fn test_build_reply_body() {
        let msg = make_test_message();
        let body = build_reply_body("Thanks!", &msg);
        assert!(body.starts_with("Thanks!"));
        assert!(body.contains("Alice Sender wrote:"));
        assert!(body.contains("> Hello,"));
        assert!(body.contains("> This is the original message."));
        assert!(body.contains("> Best regards."));
    }

    #[test]
    fn test_build_forward_body() {
        let msg = make_test_message();
        let body = build_forward_body("FYI", &msg);
        assert!(body.starts_with("FYI"));
        assert!(body.contains("---------- Forwarded message ----------"));
        assert!(body.contains("From: sender@example.com"));
        assert!(body.contains("Subject: Original subject"));
        assert!(body.contains("This is the original message."));
    }

    #[test]
    fn test_build_reply_recipients_single() {
        let msg = make_test_message();
        let (to, cc) = build_reply_recipients(&msg, "me@example.com", false);
        assert_eq!(to, vec!["sender@example.com"]);
        assert!(cc.is_empty());
    }

    #[test]
    fn test_build_reply_recipients_reply_all() {
        let msg = make_test_message();
        let (to, cc) = build_reply_recipients(&msg, "me@example.com", true);
        // To should include from + original to minus self
        assert_eq!(to, vec!["sender@example.com", "bob@example.com"]);
        // CC should include original CC
        assert_eq!(cc, vec!["carol@example.com"]);
    }

    #[test]
    fn test_build_reply_recipients_excludes_self_from_cc() {
        let mut msg = make_test_message();
        msg.cc_addresses = Some(r#"["me@example.com","carol@example.com"]"#.into());
        let (to, cc) = build_reply_recipients(&msg, "me@example.com", true);
        assert!(!cc.contains(&"me@example.com".to_string()));
        assert!(cc.contains(&"carol@example.com".to_string()));
        assert!(!to.contains(&"me@example.com".to_string()));
    }

    #[test]
    fn test_build_references() {
        // Both present
        assert_eq!(
            build_references(Some("<a@x.com>"), Some("<b@x.com>")),
            Some("<a@x.com> <b@x.com>".into())
        );
        // Only message_id
        assert_eq!(
            build_references(None, Some("<b@x.com>")),
            Some("<b@x.com>".into())
        );
        // Only refs
        assert_eq!(
            build_references(Some("<a@x.com>"), None),
            Some("<a@x.com>".into())
        );
        // Neither
        assert_eq!(build_references(None, None), None);
    }

    #[test]
    fn test_format_date() {
        assert_eq!(format_date(None), "unknown date");
        // 1700000000 = Tue, Nov 14, 2023
        let d = format_date(Some(1700000000));
        assert!(d.contains("2023"));
        assert!(d.contains("Nov"));
    }
}
