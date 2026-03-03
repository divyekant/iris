use futures::TryStreamExt;

use crate::db::DbPool;
use crate::imap::connection::{connect, ImapCredentials};
use crate::models::account::Account;
use crate::models::message::InsertMessage;
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
                message_id: msg_id,
            });

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

    // Snippet: first 200 chars of body text
    let snippet = body_text.as_ref().map(|text| {
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        clean
    });

    // Check flags for \Seen
    let is_read = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Seen));

    // Check for \Flagged → starred
    let is_starred = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Flagged));

    // Check for \Draft
    let is_draft = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Draft));

    InsertMessage {
        account_id: account_id.to_string(),
        message_id,
        thread_id: None,
        folder: "INBOX".to_string(),
        from_address,
        from_name,
        to_addresses,
        cc_addresses,
        bcc_addresses: None,
        subject,
        date,
        snippet,
        body_text,
        body_html: None,
        is_read,
        is_starred,
        is_draft,
        labels: None,
        uid: fetch.uid.map(|u| u as i64),
        modseq: fetch.modseq.map(|m| m as i64),
        raw_headers,
        has_attachments: false, // TODO: parse BODYSTRUCTURE for attachments
        attachment_names: None,
        size_bytes: fetch.size.map(|s| s as i64),
    }
}
