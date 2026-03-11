use lettre::message::header::{HeaderName, HeaderValue};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::models::account::Account;
use crate::models::message::MessageDetail;

#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    #[error("SMTP not configured for this account")]
    NotConfigured,
    #[error("no access token available")]
    NoAccessToken,
    #[error("failed to build email: {0}")]
    Build(String),
    #[error("SMTP send failed: {0}")]
    Send(String),
    #[error("token refresh failed: {0}")]
    TokenRefresh(String),
}

/// Request to compose and send an email.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ComposeRequest {
    pub account_id: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    /// Set for replies — the Message-ID being replied to.
    pub in_reply_to: Option<String>,
    /// Set for replies — the References chain.
    pub references: Option<String>,
}

/// Build an RFC 2822 email message from a compose request.
pub fn build_email(
    from_email: &str,
    from_name: Option<&str>,
    req: &ComposeRequest,
) -> Result<Message, SmtpError> {
    let from_mailbox: Mailbox = if let Some(name) = from_name {
        format!("{name} <{from_email}>").parse()
    } else {
        from_email.parse()
    }
    .map_err(|e| SmtpError::Build(format!("invalid from address: {e}")))?;

    let mut builder = Message::builder()
        .from(from_mailbox)
        .subject(&req.subject);

    for to in &req.to {
        let mb: Mailbox = to
            .parse()
            .map_err(|e| SmtpError::Build(format!("invalid to address '{to}': {e}")))?;
        builder = builder.to(mb);
    }
    for cc in &req.cc {
        let mb: Mailbox = cc
            .parse()
            .map_err(|e| SmtpError::Build(format!("invalid cc address '{cc}': {e}")))?;
        builder = builder.cc(mb);
    }
    for bcc in &req.bcc {
        let mb: Mailbox = bcc
            .parse()
            .map_err(|e| SmtpError::Build(format!("invalid bcc address '{bcc}': {e}")))?;
        builder = builder.bcc(mb);
    }

    if let Some(ref reply_to) = req.in_reply_to {
        builder = builder.in_reply_to(reply_to.to_string());
    }
    if let Some(ref refs) = req.references {
        builder = builder.references(refs.to_string());
    }

    let email = if let Some(ref html) = req.body_html {
        builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(req.body_text.clone()))
                    .singlepart(SinglePart::html(html.clone())),
            )
            .map_err(|e| SmtpError::Build(e.to_string()))?
    } else {
        builder
            .body(req.body_text.clone())
            .map_err(|e| SmtpError::Build(e.to_string()))?
    };

    Ok(email)
}

/// Build a redirected/bounced email that preserves the original sender info
/// and adds RFC 2822 Resent-* headers.
pub fn build_redirect_email(
    resent_from_email: &str,
    resent_to_email: &str,
    original: &MessageDetail,
) -> Result<Message, SmtpError> {
    // Parse addresses
    let from_addr: Mailbox = if let (Some(name), Some(email)) = (&original.from_name, &original.from_address) {
        format!("{name} <{email}>").parse()
    } else if let Some(ref email) = original.from_address {
        email.parse()
    } else {
        return Err(SmtpError::Build("original message has no from address".into()));
    }
    .map_err(|e| SmtpError::Build(format!("invalid original from address: {e}")))?;

    let to_mailbox: Mailbox = resent_to_email
        .parse()
        .map_err(|e| SmtpError::Build(format!("invalid redirect-to address: {e}")))?;

    let resent_from_mailbox: Mailbox = resent_from_email
        .parse()
        .map_err(|e| SmtpError::Build(format!("invalid resent-from address: {e}")))?;

    let subject = original.subject.as_deref().unwrap_or("(no subject)");

    // Build the message preserving original From + subject, with Resent-* headers
    let mut builder = Message::builder()
        .from(from_addr)
        .to(to_mailbox.clone())
        .subject(subject);

    // Set original Date if available
    if let Some(ts) = original.date {
        use chrono::{DateTime, Utc};
        if let Some(dt) = DateTime::from_timestamp(ts, 0) {
            let dt_utc: DateTime<Utc> = dt;
            builder = builder.date(dt_utc.into());
        }
    }

    // Add Resent-* headers (RFC 2822 Section 3.6.6)
    let now = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S %z").to_string();
    builder = builder
        .raw_header(HeaderValue::new(
            HeaderName::new_from_ascii_str("Resent-From"),
            resent_from_mailbox.to_string(),
        ))
        .raw_header(HeaderValue::new(
            HeaderName::new_from_ascii_str("Resent-To"),
            to_mailbox.to_string(),
        ))
        .raw_header(HeaderValue::new(
            HeaderName::new_from_ascii_str("Resent-Date"),
            now,
        ));

    // Build body — prefer HTML with text fallback
    let email = if let Some(ref html) = original.body_html {
        let text = original.body_text.clone().unwrap_or_default();
        builder
            .multipart(
                MultiPart::alternative()
                    .singlepart(SinglePart::plain(text))
                    .singlepart(SinglePart::html(html.clone())),
            )
            .map_err(|e| SmtpError::Build(e.to_string()))?
    } else {
        let text = original.body_text.clone().unwrap_or_default();
        builder
            .body(text)
            .map_err(|e| SmtpError::Build(e.to_string()))?
    };

    Ok(email)
}

/// Send an email via SMTP using the account's configured transport.
pub async fn send_email(
    account: &Account,
    access_token: Option<&str>,
    email: Message,
) -> Result<(), SmtpError> {
    let smtp_host = account
        .smtp_host
        .as_deref()
        .ok_or(SmtpError::NotConfigured)?;
    let smtp_port = account.smtp_port.unwrap_or(587) as u16;

    let mailer = match account.provider.as_str() {
        "gmail" | "outlook" => {
            let token = access_token.ok_or(SmtpError::NoAccessToken)?;
            let creds = Credentials::new(account.email.clone(), token.to_string());
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host)
                .map_err(|e| SmtpError::Send(e.to_string()))?
                .port(smtp_port)
                .credentials(creds)
                .authentication(vec![Mechanism::Xoauth2])
                .build()
        }
        _ => {
            // Password-based auth for manual IMAP accounts
            let username = account.username.as_deref().unwrap_or(&account.email);
            let password = account.password_encrypted.as_deref().unwrap_or("");
            let creds = Credentials::new(username.to_string(), password.to_string());
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host)
                .map_err(|e| SmtpError::Send(e.to_string()))?
                .port(smtp_port)
                .credentials(creds)
                .build()
        }
    };

    mailer
        .send(email)
        .await
        .map_err(|e| SmtpError::Send(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_email_plain_text() {
        let req = ComposeRequest {
            account_id: "acct-1".into(),
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Hello".into(),
            body_text: "Hi Alice!".into(),
            body_html: None,
            in_reply_to: None,
            references: None,
        };
        let email = build_email("bob@example.com", Some("Bob"), &req).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);
        assert!(formatted.contains("From: Bob <bob@example.com>"));
        assert!(formatted.contains("To: alice@example.com"));
        assert!(formatted.contains("Subject: Hello"));
        assert!(formatted.contains("Hi Alice!"));
    }

    #[test]
    fn test_build_email_html() {
        let req = ComposeRequest {
            account_id: "acct-1".into(),
            to: vec!["alice@example.com".into()],
            cc: vec!["carol@example.com".into()],
            bcc: vec![],
            subject: "HTML email".into(),
            body_text: "Plain fallback".into(),
            body_html: Some("<p>Rich content</p>".into()),
            in_reply_to: None,
            references: None,
        };
        let email = build_email("bob@example.com", None, &req).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);
        assert!(formatted.contains("Cc: carol@example.com"));
        assert!(formatted.contains("multipart/alternative"));
    }

    #[test]
    fn test_build_email_reply_headers() {
        let req = ComposeRequest {
            account_id: "acct-1".into(),
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Re: Original".into(),
            body_text: "My reply".into(),
            body_html: None,
            in_reply_to: Some("<orig-123@example.com>".into()),
            references: Some("<orig-123@example.com>".into()),
        };
        let email = build_email("bob@example.com", None, &req).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);
        assert!(formatted.contains("In-Reply-To: <orig-123@example.com>"));
        assert!(formatted.contains("References: <orig-123@example.com>"));
    }

    #[test]
    fn test_build_redirect_email_plain_text() {
        let original = MessageDetail {
            id: "msg-1".into(),
            message_id: Some("<orig@example.com>".into()),
            account_id: "acct-1".into(),
            thread_id: None,
            folder: "INBOX".into(),
            from_address: Some("sender@example.com".into()),
            from_name: Some("Original Sender".into()),
            to_addresses: Some(r#"["me@example.com"]"#.into()),
            cc_addresses: None,
            subject: Some("Important Update".into()),
            snippet: Some("Preview...".into()),
            date: Some(1700000000),
            body_text: Some("This is the original body.".into()),
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
        };

        let email = build_redirect_email("me@example.com", "target@example.com", &original).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);

        // Preserves original From (lettre quotes display names)
        assert!(formatted.contains("From: \"Original Sender\" <sender@example.com>"));
        // Sets redirect target as To
        assert!(formatted.contains("To: target@example.com"));
        // Preserves subject
        assert!(formatted.contains("Subject: Important Update"));
        // Has Resent-From header
        assert!(formatted.contains("Resent-From: me@example.com"));
        // Has Resent-To header
        assert!(formatted.contains("Resent-To: target@example.com"));
        // Has Resent-Date header
        assert!(formatted.contains("Resent-Date:"));
        // Body preserved
        assert!(formatted.contains("This is the original body."));
    }

    #[test]
    fn test_build_redirect_email_html() {
        let original = MessageDetail {
            id: "msg-2".into(),
            message_id: None,
            account_id: "acct-1".into(),
            thread_id: None,
            folder: "INBOX".into(),
            from_address: Some("html-sender@example.com".into()),
            from_name: None,
            to_addresses: None,
            cc_addresses: None,
            subject: Some("HTML Message".into()),
            snippet: None,
            date: None,
            body_text: Some("Plain text".into()),
            body_html: Some("<p>Rich HTML</p>".into()),
            is_read: false,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
        };

        let email = build_redirect_email("me@example.com", "target@example.com", &original).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);

        assert!(formatted.contains("multipart/alternative"));
        assert!(formatted.contains("Resent-From: me@example.com"));
    }

    #[test]
    fn test_build_redirect_email_no_from_address() {
        let original = MessageDetail {
            id: "msg-3".into(),
            message_id: None,
            account_id: "acct-1".into(),
            thread_id: None,
            folder: "INBOX".into(),
            from_address: None,
            from_name: None,
            to_addresses: None,
            cc_addresses: None,
            subject: None,
            snippet: None,
            date: None,
            body_text: None,
            body_html: None,
            is_read: false,
            is_starred: false,
            has_attachments: false,
            attachments: vec![],
            ai_intent: None,
            ai_priority_score: None,
            ai_priority_label: None,
            ai_category: None,
            ai_summary: None,
        };

        let result = build_redirect_email("me@example.com", "target@example.com", &original);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no from address"));
    }
}
