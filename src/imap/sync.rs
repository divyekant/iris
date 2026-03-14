use futures::TryStreamExt;
use mailparse::MailHeaderMap;

use crate::db::DbPool;
use crate::imap::connection::{connect, ImapCredentials};
use crate::jobs::queue::{self, MemoriesStorePayload};
use crate::models::account::Account;
use crate::models::blocked_sender::BlockedSender;
use crate::models::message::{AttachmentMeta, InsertMessage};
use crate::ws::hub::{WsEvent, WsHub};

// ---------------------------------------------------------------------------
// Sync engine
// ---------------------------------------------------------------------------

/// Drives the initial (and incremental re-) sync of a single account.
pub struct SyncEngine {
    pub db: DbPool,
    pub ws_hub: WsHub,
}

impl SyncEngine {
    pub fn new(db: DbPool, ws_hub: WsHub) -> Self {
        Self { db, ws_hub }
    }

    /// Perform an initial sync: fetch the newest batch of messages from
    /// INBOX and other standard folders, inserting them into the local database.
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
            if let Ok(conn) = self.db.get() {
                Account::update_sync_status(&conn, account_id, "error", Some(&e.to_string()));
            }
            e
        })?;

        // Folders to sync: (IMAP name, local folder name, batch size)
        // Gmail uses [Gmail]/Sent Mail etc; standard IMAP uses Sent, Drafts, Trash, etc.
        // For each logical folder we try the Gmail name first, then fall back to
        // standard IMAP names. Because we skip folders that don't exist, at most
        // one entry per logical folder will succeed on a given provider.
        let folders_to_sync: Vec<(&str, &str, u32)> = vec![
            // Inbox
            ("INBOX", "INBOX", 50),
            // Sent
            ("[Gmail]/Sent Mail", "Sent", 25),
            ("Sent", "Sent", 25),
            // Trash
            ("[Gmail]/Trash", "Trash", 25),
            ("Trash", "Trash", 25),
            // Drafts
            ("[Gmail]/Drafts", "Drafts", 25),
            ("Drafts", "Drafts", 25),
            // Archive (Gmail "All Mail" contains everything — keep batch small to avoid duplicates)
            ("[Gmail]/All Mail", "Archive", 25),
            ("Archive", "Archive", 25),
            // Spam
            ("[Gmail]/Spam", "Spam", 25),
            ("Spam", "Spam", 25),
            ("Junk", "Spam", 25),
        ];

        // Track which local folders we've already synced so we don't
        // fetch the same logical folder twice (e.g. Gmail name succeeds,
        // skip the standard fallback).
        let mut synced_folders: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (imap_folder, local_folder, batch_size) in &folders_to_sync {
            // Skip if we already synced this logical folder via a provider-specific name
            // (e.g. "[Gmail]/Sent Mail" succeeded, so skip "Sent")
            if synced_folders.contains(*local_folder) {
                tracing::debug!(account_id, imap_folder, local_folder, "Already synced via another name, skipping");
                continue;
            }

            // SELECT the folder — skip if it doesn't exist (different providers)
            let mailbox = match session.select(*imap_folder).await {
                Ok(mb) => mb,
                Err(_) => {
                    tracing::debug!(account_id, imap_folder, "Folder not available, skipping");
                    continue;
                }
            };

            // Mark this logical folder as synced so we skip fallback names
            synced_folders.insert(local_folder.to_string());

            let total = mailbox.exists;
            tracing::info!(account_id, imap_folder, local_folder, total, "{} has {} messages", imap_folder, total);

            if total == 0 {
                continue;
            }

            let start = if total > *batch_size { total - batch_size + 1 } else { 1 };
            let range = format!("{}:{}", start, total);

            tracing::info!(account_id, imap_folder, range, "Fetching message range");

            let fetch_query = "(UID FLAGS ENVELOPE BODY.PEEK[TEXT] RFC822.SIZE BODY.PEEK[HEADER])";
            let fetches: Vec<_> = match session.fetch(&range, fetch_query).await {
                Ok(stream) => match stream.try_collect().await {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!(account_id, imap_folder, error = %e, "Fetch failed for folder");
                        continue;
                    }
                },
                Err(e) => {
                    tracing::warn!(account_id, imap_folder, error = %e, "Fetch command failed for folder");
                    continue;
                }
            };

            let fetched_count = fetches.len();
            tracing::info!(account_id, imap_folder, fetched_count, "Fetched {} messages", fetched_count);

            for (i, fetch) in fetches.iter().enumerate() {
                let (mut insert_msg, extracted_attachments) = parse_fetch(account_id, fetch);
                insert_msg.folder = local_folder.to_string();

                let conn = self.db.get()?;
                let msg_id = InsertMessage::insert(&conn, &insert_msg);

                // Only process newly inserted messages (skip duplicates)
                if let Some(ref msg_id) = msg_id {
                    // Check if the sender is blocked — auto-move to Spam
                    let sender_blocked = insert_msg
                        .from_address
                        .as_deref()
                        .map(|addr| BlockedSender::is_blocked(&conn, addr))
                        .unwrap_or(false);

                    if sender_blocked {
                        conn.execute(
                            "UPDATE messages SET folder = 'Spam', updated_at = unixepoch() WHERE id = ?1",
                            rusqlite::params![msg_id],
                        )
                        .ok();
                        tracing::info!(
                            account_id,
                            msg_id,
                            from = ?insert_msg.from_address,
                            "Blocked sender: auto-moved to Spam"
                        );
                        // Skip inbox broadcast and AI jobs for blocked messages
                    } else {
                        // Store attachment data in the attachments table
                        if !extracted_attachments.is_empty() {
                            for att in &extracted_attachments {
                                let att_id = uuid::Uuid::new_v4().to_string();
                                let _ = conn.execute(
                                    "INSERT INTO attachments (id, message_id, filename, content_type, size, content_id, data)
                                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                                    rusqlite::params![
                                        att_id,
                                        msg_id,
                                        att.filename,
                                        att.content_type,
                                        att.size as i64,
                                        att.content_id,
                                        att.data,
                                    ],
                                );
                            }
                        }
                        // Broadcast new email event (only for INBOX)
                        if *local_folder == "INBOX" {
                            self.ws_hub.broadcast(WsEvent::NewEmail {
                                account_id: account_id.to_string(),
                                message_id: msg_id.clone(),
                            });
                        }

                        // Enqueue AI classification and Memories storage jobs
                        {
                            queue::enqueue_ai_classify(
                                &conn,
                                msg_id,
                                &insert_msg.subject.clone().unwrap_or_default(),
                                &insert_msg.from_address.clone().unwrap_or_default(),
                                &insert_msg.body_text.clone().unwrap_or_default(),
                            );
                            queue::enqueue_memories_store(
                                &conn,
                                msg_id,
                                &MemoriesStorePayload {
                                    account_id: account_id.to_string(),
                                    rfc_message_id: insert_msg.message_id.clone(),
                                    from_name: insert_msg.from_name.clone(),
                                    from_address: insert_msg.from_address.clone(),
                                    subject: insert_msg.subject.clone(),
                                    body_text: insert_msg.body_text.clone(),
                                    date: insert_msg.date,
                                },
                            );
                        }
                    }
                }

                // Broadcast progress
                let progress = (i + 1) as f32 / fetched_count as f32;
                self.ws_hub.broadcast(WsEvent::SyncStatus {
                    account_id: account_id.to_string(),
                    status: "syncing".to_string(),
                    progress: Some(progress),
                });
            }
        }

        // Done
        {
            let conn = self.db.get()?;
            Account::update_sync_status(&conn, account_id, "idle", None);
            // Refresh inbox stats after sync
            if let Err(e) = crate::api::inbox_stats::compute_and_store(&conn, account_id) {
                tracing::warn!(account_id, error = %e, "Failed to update inbox stats");
            }
        }
        self.ws_hub.broadcast(WsEvent::SyncComplete {
            account_id: account_id.to_string(),
        });

        let _ = session.logout().await;

        tracing::info!(account_id, "Initial sync complete");
        Ok(())
    }

}

// ---------------------------------------------------------------------------
// RFC 2047 encoded-word decoding
// ---------------------------------------------------------------------------

/// Decode RFC 2047 encoded-words (e.g. `=?UTF-8?Q?Hello?=`) using mailparse.
/// Falls back to lossy UTF-8 conversion if decoding fails.
fn decode_rfc2047(raw: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(raw).to_string();
    if !lossy.contains("=?") {
        return lossy;
    }
    // Build a fake header line so mailparse can decode the encoded-words
    let header_line = format!("Subject: {}", lossy);
    match mailparse::parse_header(header_line.as_bytes()) {
        Ok((header, _)) => header.get_value(),
        Err(_) => lossy,
    }
}

// ---------------------------------------------------------------------------
// Parse a single FETCH response into an InsertMessage
// ---------------------------------------------------------------------------

fn parse_fetch(account_id: &str, fetch: &async_imap::types::Fetch) -> (InsertMessage, Vec<ExtractedAttachment>) {
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
                        .map(|n| decode_rfc2047(n));
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

    // Extract subject (decode RFC 2047 encoded-words)
    let subject = envelope.and_then(|env| {
        env.subject
            .as_ref()
            .map(|s| decode_rfc2047(s))
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
            attachment_data: Vec::new(),
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

    // Parse Gmail labels from X-Gmail-Labels header (if present)
    let labels = raw_headers
        .as_deref()
        .and_then(extract_gmail_labels);

    // Parse List-Unsubscribe and List-Unsubscribe-Post headers (RFC 2369 / RFC 8058)
    let (list_unsubscribe, list_unsubscribe_post) = raw_headers
        .as_deref()
        .map(|h| {
            let headers = match mailparse::parse_headers(h.as_bytes()) {
                Ok((hdrs, _)) => hdrs,
                Err(_) => return (None, false),
            };
            let unsub_url = headers
                .get_first_value("List-Unsubscribe")
                .and_then(|v| parse_list_unsubscribe(&v));
            let has_post = headers
                .get_first_value("List-Unsubscribe-Post")
                .is_some();
            (unsub_url, has_post)
        })
        .unwrap_or((None, false));

    (InsertMessage {
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
        labels,
        uid: fetch.uid.map(|u| u as i64),
        modseq: fetch.modseq.map(|m| m as i64),
        raw_headers,
        has_attachments,
        attachment_names,
        size_bytes: fetch.size.map(|s| s as i64),
        list_unsubscribe,
        list_unsubscribe_post,
    }, mime_parsed.attachment_data)
}

// ---------------------------------------------------------------------------
// MIME parsing
// ---------------------------------------------------------------------------

/// Raw attachment data extracted from MIME parts, ready for DB storage.
#[derive(Debug, Clone)]
pub struct ExtractedAttachment {
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub content_id: Option<String>,
    pub data: Vec<u8>,
}

struct ParsedBody {
    text: Option<String>,
    html: Option<String>,
    attachments: Vec<AttachmentMeta>,
    attachment_data: Vec<ExtractedAttachment>,
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
                attachment_data: Vec::new(),
            };
        }
    };

    let mut text = None;
    let mut html = None;
    let mut attachments = Vec::new();
    let mut attachment_data = Vec::new();

    extract_mime_parts(&parsed, &mut text, &mut html, &mut attachments, &mut attachment_data);

    ParsedBody {
        text,
        html,
        attachments,
        attachment_data,
    }
}

/// Recursively walk MIME parts to extract text, HTML, and attachment metadata + data.
fn extract_mime_parts(
    part: &mailparse::ParsedMail,
    text: &mut Option<String>,
    html: &mut Option<String>,
    attachments: &mut Vec<AttachmentMeta>,
    attachment_data: &mut Vec<ExtractedAttachment>,
) {
    let content_type = &part.ctype;
    let disposition = part.get_content_disposition();

    // Check if this part is an attachment (explicit attachment disposition or
    // inline with Content-ID for embedded images)
    let is_attachment = disposition.disposition == mailparse::DispositionType::Attachment;
    let is_inline_image = disposition.disposition == mailparse::DispositionType::Inline
        && content_type.mimetype.starts_with("image/")
        && part.headers.iter().any(|h| h.get_key_ref().eq_ignore_ascii_case("Content-ID"));

    if is_attachment || is_inline_image {
        let filename = disposition
            .params
            .get("filename")
            .or_else(|| content_type.params.get("name"))
            .cloned()
            .unwrap_or_else(|| "unnamed".to_string());
        let mime_type = content_type.mimetype.clone();
        let raw_data = part.get_body_raw().unwrap_or_default();
        let size = raw_data.len();

        // Extract Content-ID for inline images
        let content_id = part.headers.iter()
            .find(|h| h.get_key_ref().eq_ignore_ascii_case("Content-ID"))
            .map(|h| h.get_value().trim_matches(|c| c == '<' || c == '>').to_string());

        attachments.push(AttachmentMeta {
            filename: filename.clone(),
            mime_type: mime_type.clone(),
            size,
        });
        attachment_data.push(ExtractedAttachment {
            filename,
            content_type: mime_type,
            size,
            content_id,
            data: raw_data,
        });
        return;
    }

    // If this part has subparts, recurse into them
    if !part.subparts.is_empty() {
        for subpart in &part.subparts {
            extract_mime_parts(subpart, text, html, attachments, attachment_data);
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
// List-Unsubscribe parsing (RFC 2369 / RFC 8058)
// ---------------------------------------------------------------------------

/// Parse the List-Unsubscribe header value and extract the best URL.
/// Prefers https:// over http:// over mailto: URLs.
/// URLs are enclosed in angle brackets, comma-separated:
/// e.g. `<https://example.com/unsub>, <mailto:unsub@example.com>`
fn parse_list_unsubscribe(header_value: &str) -> Option<String> {
    // First pass: prefer HTTPS URLs
    for part in header_value.split(',') {
        let trimmed = part.trim();
        if let Some(url) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            if url.starts_with("https://") {
                return Some(url.to_string());
            }
        }
    }
    // Second pass: accept HTTP URLs
    for part in header_value.split(',') {
        let trimmed = part.trim();
        if let Some(url) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            if url.starts_with("http://") {
                return Some(url.to_string());
            }
        }
    }
    // Third pass: fall back to mailto
    for part in header_value.split(',') {
        let trimmed = part.trim();
        if let Some(url) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
            if url.starts_with("mailto:") {
                return Some(url.to_string());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Gmail label extraction
// ---------------------------------------------------------------------------

/// Extract Gmail labels from the `X-Gmail-Labels` header.
///
/// Gmail includes this header in IMAP-fetched messages with a comma-separated
/// list of labels (e.g. `X-Gmail-Labels: Important,Starred,Social`).
/// Returns a JSON array string like `["Important","Social"]` or None if the
/// header is absent or empty.
fn extract_gmail_labels(raw_headers: &str) -> Option<String> {
    let headers = mailparse::parse_headers(raw_headers.as_bytes()).ok()?.0;
    let value = headers.get_first_value("X-Gmail-Labels")?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let labels: Vec<String> = trimmed
        .split(',')
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if labels.is_empty() {
        return None;
    }
    serde_json::to_string(&labels).ok()
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

    #[test]
    fn test_decode_rfc2047_quoted_printable() {
        let encoded = b"=?UTF-8?Q?BadBo_1.0_Making_History_=F0=9F=90=B0?=";
        let decoded = decode_rfc2047(encoded);
        assert!(
            decoded.contains("BadBo 1.0 Making History"),
            "should decode QP encoded subject, got: {}",
            decoded
        );
    }

    #[test]
    fn test_decode_rfc2047_plain_ascii() {
        let plain = b"Hello World";
        assert_eq!(decode_rfc2047(plain), "Hello World");
    }

    #[test]
    fn test_decode_rfc2047_base64() {
        // "Hello" in UTF-8 Base64
        let encoded = b"=?UTF-8?B?SGVsbG8=?=";
        let decoded = decode_rfc2047(encoded);
        assert_eq!(decoded, "Hello", "should decode Base64 encoded-word");
    }

    #[test]
    fn test_decode_rfc2047_multiple_encoded_words() {
        let encoded = b"=?UTF-8?Q?Part_One?= =?UTF-8?Q?_Part_Two?=";
        let decoded = decode_rfc2047(encoded);
        assert!(
            decoded.contains("Part One") && decoded.contains("Part Two"),
            "should decode multiple encoded-words, got: {}",
            decoded
        );
    }

    #[test]
    fn test_extract_gmail_labels_present() {
        let headers = "From: a@b.com\r\nX-Gmail-Labels: Important,Social,Updates\r\nSubject: Hi\r\n";
        let result = extract_gmail_labels(headers);
        assert!(result.is_some());
        let labels: Vec<String> = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(labels, vec!["Important", "Social", "Updates"]);
    }

    #[test]
    fn test_extract_gmail_labels_with_spaces() {
        let headers = "X-Gmail-Labels: Promotions , Forums , Starred\r\n";
        let result = extract_gmail_labels(headers);
        assert!(result.is_some());
        let labels: Vec<String> = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(labels, vec!["Promotions", "Forums", "Starred"]);
    }

    #[test]
    fn test_extract_gmail_labels_absent() {
        let headers = "From: a@b.com\r\nSubject: No labels\r\n";
        let result = extract_gmail_labels(headers);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_gmail_labels_empty() {
        let headers = "X-Gmail-Labels: \r\n";
        let result = extract_gmail_labels(headers);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_gmail_labels_single() {
        let headers = "X-Gmail-Labels: Inbox\r\n";
        let result = extract_gmail_labels(headers);
        assert!(result.is_some());
        let labels: Vec<String> = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(labels, vec!["Inbox"]);
    }

    #[test]
    fn test_parse_list_unsubscribe_http() {
        let header = "<https://example.com/unsubscribe?id=123>";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, Some("https://example.com/unsubscribe?id=123".to_string()));
    }

    #[test]
    fn test_parse_list_unsubscribe_mailto() {
        let header = "<mailto:unsubscribe@example.com?subject=Unsubscribe>";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, Some("mailto:unsubscribe@example.com?subject=Unsubscribe".to_string()));
    }

    #[test]
    fn test_parse_list_unsubscribe_both_prefers_http() {
        let header = "<mailto:unsub@example.com>, <https://example.com/unsub>";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, Some("https://example.com/unsub".to_string()));
    }

    #[test]
    fn test_parse_list_unsubscribe_empty() {
        let header = "";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_list_unsubscribe_http_fallback() {
        let header = "<http://example.com/unsub>";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, Some("http://example.com/unsub".to_string()));
    }

    #[test]
    fn test_parse_list_unsubscribe_https_preferred_over_http() {
        let header = "<http://example.com/unsub>, <https://example.com/unsub-secure>";
        let result = parse_list_unsubscribe(header);
        assert_eq!(result, Some("https://example.com/unsub-secure".to_string()));
    }
}
