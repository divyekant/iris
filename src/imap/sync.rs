use futures::TryStreamExt;
use mailparse::MailHeaderMap;

use crate::ai::ollama::OllamaClient;
use crate::ai::pipeline;
use crate::db::DbPool;
use crate::imap::connection::{connect, ImapCredentials};
use crate::models::account::Account;
use crate::models::message::{self, AttachmentMeta, InsertMessage};
use crate::ws::hub::{WsEvent, WsHub};

// ---------------------------------------------------------------------------
// Sync engine
// ---------------------------------------------------------------------------

/// Drives the initial (and incremental re-) sync of a single account.
pub struct SyncEngine {
    pub db: DbPool,
    pub ws_hub: WsHub,
    pub ollama: OllamaClient,
}

impl SyncEngine {
    /// Perform an initial sync: fetch the newest batch of messages from
    /// INBOX and insert them into the local database.
    pub async fn initial_sync(
        &self,
        account_id: &str,
        creds: &ImapCredentials,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 1. Mark account as syncing
        {
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "syncing", None);
        }
        self.ws_hub.broadcast(WsEvent::SyncStatus {
            account_id: account_id.to_string(),
            status: "syncing".to_string(),
            progress: Some(0.0),
        });

        // 2. Connect to IMAP
        let mut session = connect(creds).await.map_err(|e| {
            // Update status on connection failure
            if let Ok(conn) = self.db.get() {
                Account::update_sync_status(&conn, account_id, "error", Some(&e.to_string()));
            }
            e
        })?;

        // 3. SELECT INBOX
        let mailbox = session.select("INBOX").await.map_err(|e| {
            if let Ok(conn) = self.db.get() {
                Account::update_sync_status(&conn, account_id, "error", Some(&e.to_string()));
            }
            e
        })?;

        let total = mailbox.exists;
        tracing::info!(account_id, total, "INBOX has {} messages", total);

        if total == 0 {
            // Nothing to sync
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "idle", None);
            self.ws_hub.broadcast(WsEvent::SyncComplete {
                account_id: account_id.to_string(),
            });
            let _ = session.logout().await;
            return Ok(());
        }

        // 4. Determine range: newest 100 (or fewer)
        let batch_size: u32 = 100;
        let start = if total > batch_size { total - batch_size + 1 } else { 1 };
        let range = format!("{}:{}", start, total);

        tracing::info!(account_id, range, "Fetching message range");

        // 5. FETCH
        let fetch_query = "(UID FLAGS ENVELOPE BODY.PEEK[TEXT] RFC822.SIZE BODY.PEEK[HEADER])";
        let fetches: Vec<_> = session
            .fetch(&range, fetch_query)
            .await?
            .try_collect()
            .await?;

        let fetched_count = fetches.len();
        tracing::info!(account_id, fetched_count, "Fetched {} messages", fetched_count);

        // 6. Parse and insert each message
        for (i, fetch) in fetches.iter().enumerate() {
            let insert_msg = parse_fetch(account_id, fetch);

            let msg_id = {
                let conn = self.db.get()?;
                InsertMessage::insert(&conn, &insert_msg)
            };

            // Broadcast new email event
            self.ws_hub.broadcast(WsEvent::NewEmail {
                account_id: account_id.to_string(),
                message_id: msg_id.clone(),
            });

            // Spawn AI processing in background (fire-and-forget)
            self.spawn_ai_processing(
                msg_id,
                insert_msg.subject.clone().unwrap_or_default(),
                insert_msg.from_address.clone().unwrap_or_default(),
                insert_msg.body_text.clone().unwrap_or_default(),
            );

            // Broadcast progress
            let progress = (i + 1) as f32 / fetched_count as f32;
            self.ws_hub.broadcast(WsEvent::SyncStatus {
                account_id: account_id.to_string(),
                status: "syncing".to_string(),
                progress: Some(progress),
            });
        }

        // 7. Done
        {
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "idle", None);
        }
        self.ws_hub.broadcast(WsEvent::SyncComplete {
            account_id: account_id.to_string(),
        });

        let _ = session.logout().await;

        tracing::info!(account_id, "Initial sync complete");
        Ok(())
    }

    /// Spawn a background task to process a message through the AI pipeline.
    /// Gracefully degrades: if AI is not configured or Ollama is down, does nothing.
    fn spawn_ai_processing(
        &self,
        message_id: String,
        subject: String,
        from: String,
        body: String,
    ) {
        let db = self.db.clone();
        let ws_hub = self.ws_hub.clone();
        let ollama = self.ollama.clone();

        tokio::spawn(async move {
            // Check if AI is enabled
            let (enabled, model) = {
                let conn = match db.get() {
                    Ok(c) => c,
                    Err(_) => return,
                };
                let enabled = conn
                    .query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                    .unwrap_or_else(|_| "false".to_string())
                    == "true";
                let model = conn
                    .query_row("SELECT value FROM config WHERE key = 'ai_model'", [], |row| row.get::<_, String>(0))
                    .unwrap_or_default();
                (enabled, model)
            };

            if !enabled || model.is_empty() {
                return;
            }

            // Run AI pipeline
            let metadata = match pipeline::process_email(&ollama, &model, &subject, &from, &body).await {
                Some(m) => m,
                None => return,
            };

            // Update message with AI metadata
            if let Ok(conn) = db.get() {
                let updated = message::update_ai_metadata(
                    &conn,
                    &message_id,
                    &metadata.intent,
                    metadata.priority_score,
                    &metadata.priority_label,
                    &metadata.category,
                    &metadata.summary,
                );
                if updated {
                    ws_hub.broadcast(WsEvent::AiProcessed {
                        message_id: message_id.clone(),
                    });
                    tracing::debug!(message_id, "AI processing complete");
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Parse a single FETCH response into an InsertMessage
// ---------------------------------------------------------------------------

fn parse_fetch(account_id: &str, fetch: &async_imap::types::Fetch) -> InsertMessage {
    let envelope = fetch.envelope();

    // Extract from address
    let (from_address, from_name) = envelope
        .and_then(|env| {
            env.from.as_ref().and_then(|addrs| {
                addrs.first().map(|addr| {
                    let mailbox = addr
                        .mailbox
                        .as_ref()
                        .map(|m| String::from_utf8_lossy(m).to_string())
                        .unwrap_or_default();
                    let host = addr
                        .host
                        .as_ref()
                        .map(|h| String::from_utf8_lossy(h).to_string())
                        .unwrap_or_default();
                    let email = format!("{}@{}", mailbox, host);
                    let name = addr
                        .name
                        .as_ref()
                        .map(|n| String::from_utf8_lossy(n).to_string());
                    (Some(email), name)
                })
            })
        })
        .unwrap_or((None, None));

    // Extract to addresses
    let to_addresses = envelope.and_then(|env| {
        env.to.as_ref().map(|addrs| {
            let list: Vec<String> = addrs
                .iter()
                .filter_map(|addr| {
                    let mailbox = addr.mailbox.as_ref().map(|m| String::from_utf8_lossy(m).to_string())?;
                    let host = addr.host.as_ref().map(|h| String::from_utf8_lossy(h).to_string())?;
                    Some(format!("{}@{}", mailbox, host))
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_default()
        })
    });

    // Extract CC addresses
    let cc_addresses = envelope.and_then(|env| {
        env.cc.as_ref().map(|addrs| {
            let list: Vec<String> = addrs
                .iter()
                .filter_map(|addr| {
                    let mailbox = addr.mailbox.as_ref().map(|m| String::from_utf8_lossy(m).to_string())?;
                    let host = addr.host.as_ref().map(|h| String::from_utf8_lossy(h).to_string())?;
                    Some(format!("{}@{}", mailbox, host))
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_default()
        })
    });

    // Extract subject
    let subject = envelope.and_then(|env| {
        env.subject
            .as_ref()
            .map(|s| String::from_utf8_lossy(s).to_string())
    });

    // Extract message-id
    let message_id = envelope.and_then(|env| {
        env.message_id
            .as_ref()
            .map(|id| String::from_utf8_lossy(id).to_string())
    });

    // Extract date from envelope
    let date_str = envelope.and_then(|env| {
        env.date
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).to_string())
    });

    // Parse date to epoch
    let date = date_str.and_then(|ds| {
        chrono::DateTime::parse_from_rfc2822(&ds)
            .ok()
            .map(|dt| dt.timestamp())
    });

    // Extract body text
    let body_text = fetch
        .text()
        .map(|b| String::from_utf8_lossy(b).to_string());

    // Extract raw headers
    let raw_headers = fetch
        .header()
        .map(|h| String::from_utf8_lossy(h).to_string());

    // Parse MIME body for text, html, and attachments
    let mime_parsed = match (&raw_headers, &body_text) {
        (Some(headers), Some(body)) => parse_mime_body(headers, body),
        _ => ParsedBody {
            text: body_text.clone(),
            html: None,
            attachments: Vec::new(),
        },
    };

    // Extract thread ID from headers
    let thread_id = raw_headers
        .as_deref()
        .map(|h| extract_thread_id(h, &message_id))
        .unwrap_or_else(|| message_id.clone());

    // Snippet: first 200 chars of the extracted plain text body
    let snippet = mime_parsed.text.as_ref().map(|text| {
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        clean
    });

    // Serialize attachment metadata
    let has_attachments = !mime_parsed.attachments.is_empty();
    let attachment_names = if has_attachments {
        serde_json::to_string(&mime_parsed.attachments).ok()
    } else {
        None
    };

    // Check flags for \Seen
    let is_read = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Seen));

    // Check for \Flagged → starred
    let is_starred = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Flagged));

    // Check for \Draft
    let is_draft = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Draft));

    InsertMessage {
        account_id: account_id.to_string(),
        message_id,
        thread_id,
        folder: "INBOX".to_string(),
        from_address,
        from_name,
        to_addresses,
        cc_addresses,
        bcc_addresses: None,
        subject,
        date,
        snippet,
        body_text: mime_parsed.text,
        body_html: mime_parsed.html,
        is_read,
        is_starred,
        is_draft,
        labels: None,
        uid: fetch.uid.map(|u| u as i64),
        modseq: fetch.modseq.map(|m| m as i64),
        raw_headers,
        has_attachments,
        attachment_names,
        size_bytes: fetch.size.map(|s| s as i64),
    }
}

// ---------------------------------------------------------------------------
// MIME parsing
// ---------------------------------------------------------------------------

struct ParsedBody {
    text: Option<String>,
    html: Option<String>,
    attachments: Vec<AttachmentMeta>,
}

/// Parse the MIME body of a message.
/// Concatenates raw headers + body, then uses mailparse to extract
/// text/plain, text/html, and attachment metadata.
fn parse_mime_body(raw_headers: &str, raw_body: &str) -> ParsedBody {
    // MIME requires a blank line (\r\n\r\n) between headers and body.
    // Trim trailing whitespace from headers, then add the blank line separator.
    let full = format!("{}\r\n\r\n{}", raw_headers.trim_end(), raw_body);

    let parsed = match mailparse::parse_mail(full.as_bytes()) {
        Ok(p) => p,
        Err(_) => {
            // Fallback: return raw body as text
            return ParsedBody {
                text: Some(raw_body.to_string()),
                html: None,
                attachments: Vec::new(),
            };
        }
    };

    let mut text = None;
    let mut html = None;
    let mut attachments = Vec::new();

    extract_mime_parts(&parsed, &mut text, &mut html, &mut attachments);

    ParsedBody {
        text,
        html,
        attachments,
    }
}

/// Recursively walk MIME parts to extract text, HTML, and attachment metadata.
fn extract_mime_parts(
    part: &mailparse::ParsedMail,
    text: &mut Option<String>,
    html: &mut Option<String>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    let content_type = &part.ctype;
    let disposition = part.get_content_disposition();

    // Check if this part is an attachment
    if disposition.disposition == mailparse::DispositionType::Attachment {
        let filename = disposition
            .params
            .get("filename")
            .cloned()
            .unwrap_or_else(|| "unnamed".to_string());
        let mime_type = format!("{}/{}", content_type.mimetype.split('/').next().unwrap_or("application"),
            content_type.mimetype.split('/').nth(1).unwrap_or("octet-stream"));
        let size = part.get_body_raw().map(|b| b.len()).unwrap_or(0);
        attachments.push(AttachmentMeta {
            filename,
            mime_type,
            size,
        });
        return;
    }

    // If this part has subparts, recurse into them
    if !part.subparts.is_empty() {
        for subpart in &part.subparts {
            extract_mime_parts(subpart, text, html, attachments);
        }
        return;
    }

    // Leaf part: extract text or html
    let mime = &content_type.mimetype;
    if mime == "text/plain" && text.is_none() {
        if let Ok(body) = part.get_body() {
            text.replace(body);
        }
    } else if mime == "text/html" && html.is_none() {
        if let Ok(body) = part.get_body() {
            html.replace(body);
        }
    }
}

// ---------------------------------------------------------------------------
// Thread ID extraction
// ---------------------------------------------------------------------------

/// Extract a thread ID from the message headers.
///
/// Strategy:
/// 1. If `References` header exists, use the first message-id (thread root).
/// 2. Else if `In-Reply-To` header exists, use that.
/// 3. Else fall back to the message's own `Message-ID`.
///
/// All message-ids have angle brackets stripped.
fn extract_thread_id(raw_headers: &str, message_id: &Option<String>) -> Option<String> {
    let headers = match mailparse::parse_headers(raw_headers.as_bytes()) {
        Ok((headers, _)) => headers,
        Err(_) => return message_id.clone(),
    };

    // Helper to strip angle brackets from a message-id
    let strip_brackets = |s: &str| -> String {
        s.trim().trim_start_matches('<').trim_end_matches('>').to_string()
    };

    // Check References header (first ID = thread root)
    if let Some(refs) = headers.get_first_value("References") {
        let first_ref = refs.split_whitespace().next();
        if let Some(r) = first_ref {
            let stripped = strip_brackets(r);
            if !stripped.is_empty() {
                return Some(stripped);
            }
        }
    }

    // Check In-Reply-To header
    if let Some(in_reply_to) = headers.get_first_value("In-Reply-To") {
        let stripped = strip_brackets(&in_reply_to);
        if !stripped.is_empty() {
            return Some(stripped);
        }
    }

    // Fall back to own message-id
    message_id.as_ref().map(|id| strip_brackets(id))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mime_plain_text() {
        let headers = "From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nContent-Type: text/plain; charset=utf-8\r\n";
        let body = "Hello, this is a plain text email.\r\n";

        let result = parse_mime_body(headers, body);

        assert!(result.text.is_some(), "text should be extracted");
        assert!(
            result.text.as_ref().unwrap().contains("Hello, this is a plain text email."),
            "text should contain the body content"
        );
        assert!(result.html.is_none(), "html should be None for plain text");
        assert!(result.attachments.is_empty(), "no attachments expected");
    }

    #[test]
    fn test_parse_mime_multipart() {
        let headers = concat!(
            "From: sender@example.com\r\n",
            "To: recipient@example.com\r\n",
            "Subject: Multipart\r\n",
            "Content-Type: multipart/alternative; boundary=\"boundary123\"\r\n",
        );
        let body = concat!(
            "--boundary123\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Plain text version.\r\n",
            "--boundary123\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<p>HTML version.</p>\r\n",
            "--boundary123--\r\n",
        );

        let result = parse_mime_body(headers, body);

        assert!(result.text.is_some(), "text part should be extracted");
        assert!(
            result.text.as_ref().unwrap().contains("Plain text version."),
            "text should match"
        );
        assert!(result.html.is_some(), "html part should be extracted");
        assert!(
            result.html.as_ref().unwrap().contains("<p>HTML version.</p>"),
            "html should match"
        );
        assert!(result.attachments.is_empty(), "no attachments in alternative");
    }

    #[test]
    fn test_parse_mime_with_attachment() {
        let headers = concat!(
            "From: sender@example.com\r\n",
            "To: recipient@example.com\r\n",
            "Subject: With Attachment\r\n",
            "Content-Type: multipart/mixed; boundary=\"mixboundary\"\r\n",
        );
        let body = concat!(
            "--mixboundary\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "See attached file.\r\n",
            "--mixboundary\r\n",
            "Content-Type: application/pdf; name=\"report.pdf\"\r\n",
            "Content-Disposition: attachment; filename=\"report.pdf\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "JVBERi0xLjQK\r\n",
            "--mixboundary--\r\n",
        );

        let result = parse_mime_body(headers, body);

        assert!(result.text.is_some(), "text part should be extracted");
        assert!(
            result.text.as_ref().unwrap().contains("See attached file."),
            "text should match"
        );
        assert_eq!(result.attachments.len(), 1, "should have 1 attachment");
        assert_eq!(result.attachments[0].filename, "report.pdf");
        assert_eq!(result.attachments[0].mime_type, "application/pdf");
        assert!(result.attachments[0].size > 0, "attachment should have size > 0");
    }

    #[test]
    fn test_extract_thread_id_with_references() {
        let headers = concat!(
            "From: sender@example.com\r\n",
            "Message-ID: <msg3@example.com>\r\n",
            "References: <root@example.com> <msg2@example.com>\r\n",
            "In-Reply-To: <msg2@example.com>\r\n",
        );
        let message_id = Some("<msg3@example.com>".to_string());

        let thread_id = extract_thread_id(headers, &message_id);

        assert_eq!(
            thread_id,
            Some("root@example.com".to_string()),
            "should use first message-id from References as thread root"
        );
    }

    #[test]
    fn test_extract_thread_id_with_in_reply_to_only() {
        let headers = concat!(
            "From: sender@example.com\r\n",
            "Message-ID: <reply@example.com>\r\n",
            "In-Reply-To: <original@example.com>\r\n",
        );
        let message_id = Some("<reply@example.com>".to_string());

        let thread_id = extract_thread_id(headers, &message_id);

        assert_eq!(
            thread_id,
            Some("original@example.com".to_string()),
            "should use In-Reply-To when no References header"
        );
    }

    #[test]
    fn test_extract_thread_id_standalone() {
        let headers = concat!(
            "From: sender@example.com\r\n",
            "Message-ID: <standalone@example.com>\r\n",
        );
        let message_id = Some("<standalone@example.com>".to_string());

        let thread_id = extract_thread_id(headers, &message_id);

        assert_eq!(
            thread_id,
            Some("standalone@example.com".to_string()),
            "should fall back to own message-id with brackets stripped"
        );
    }
}
