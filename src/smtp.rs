use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::models::account::Account;

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

/// Base64-encoded attachment sent from the client.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data_base64: String,
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
    /// Attachments to include (base64-encoded data).
    #[serde(default)]
    pub attachments: Vec<AttachmentData>,
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

    // Build the body part (text-only or multipart/alternative with HTML)
    let body_part = if let Some(ref html) = req.body_html {
        MultiPart::alternative()
            .singlepart(SinglePart::plain(req.body_text.clone()))
            .singlepart(SinglePart::html(html.clone()))
    } else {
        MultiPart::alternative()
            .singlepart(SinglePart::plain(req.body_text.clone()))
    };

    // If we have attachments, wrap in multipart/mixed
    let email = if req.attachments.is_empty() {
        if req.body_html.is_some() {
            builder
                .multipart(body_part)
                .map_err(|e| SmtpError::Build(e.to_string()))?
        } else {
            builder
                .body(req.body_text.clone())
                .map_err(|e| SmtpError::Build(e.to_string()))?
        }
    } else {
        use base64::Engine;
        let mut mixed = MultiPart::mixed().multipart(body_part);

        for att in &req.attachments {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&att.data_base64)
                .map_err(|e| SmtpError::Build(format!("invalid base64 attachment: {e}")))?;

            let content_type: ContentType = att.content_type.parse()
                .unwrap_or(ContentType::parse("application/octet-stream").unwrap());

            let attachment = Attachment::new(att.filename.clone())
                .body(decoded, content_type);

            mixed = mixed.singlepart(attachment);
        }

        builder
            .multipart(mixed)
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
            attachments: vec![],
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
            attachments: vec![],
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
            attachments: vec![],
        };
        let email = build_email("bob@example.com", None, &req).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);
        assert!(formatted.contains("In-Reply-To: <orig-123@example.com>"));
        assert!(formatted.contains("References: <orig-123@example.com>"));
    }

    #[test]
    fn test_build_email_with_attachments() {
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.encode(b"Hello PDF content");
        let req = ComposeRequest {
            account_id: "acct-1".into(),
            to: vec!["alice@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "With attachment".into(),
            body_text: "See attached.".into(),
            body_html: None,
            in_reply_to: None,
            references: None,
            attachments: vec![AttachmentData {
                filename: "report.pdf".into(),
                content_type: "application/pdf".into(),
                data_base64: data,
            }],
        };
        let email = build_email("bob@example.com", Some("Bob"), &req).unwrap();
        let raw = email.formatted();
        let formatted = String::from_utf8_lossy(&raw);
        assert!(formatted.contains("multipart/mixed"), "should be multipart/mixed");
        assert!(formatted.contains("report.pdf"), "should contain filename");
    }
}
