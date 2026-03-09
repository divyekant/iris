use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::ai::memories::MemoriesClient;
use crate::ai::provider::ProviderPool;
use crate::ai::pipeline;
use crate::auth::refresh::ensure_fresh_token;
use crate::config::Config;
use crate::db::DbPool;
use crate::jobs::queue::{self, Job};
use crate::models::account::Account;
use crate::models::message::{self, InsertMessage};
use crate::smtp::{self, ComposeRequest};
use crate::ws::hub::{WsEvent, WsHub};

/// Background worker that polls the job queue and processes jobs.
pub struct JobWorker {
    db: DbPool,
    config: Config,
    ws_hub: WsHub,
    providers: ProviderPool,
    memories: MemoriesClient,
    poll_interval: Duration,
    semaphore: Arc<Semaphore>,
    cleanup_days: i64,
}

impl JobWorker {
    pub fn new(
        db: DbPool,
        config: Config,
        ws_hub: WsHub,
        providers: ProviderPool,
        memories: MemoriesClient,
        poll_interval_ms: u64,
        max_concurrency: usize,
        cleanup_days: i64,
    ) -> Self {
        Self {
            db,
            config,
            ws_hub,
            providers,
            memories,
            poll_interval: Duration::from_millis(poll_interval_ms),
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
            cleanup_days,
        }
    }

    /// Run the worker loop. Call this from a spawned task.
    pub async fn run(self: Arc<Self>) {
        tracing::info!("Job worker started (poll={}ms, concurrency={})",
            self.poll_interval.as_millis(),
            self.semaphore.available_permits());

        let mut iteration: u64 = 0;
        loop {
            iteration += 1;

            // Claim jobs
            let jobs = {
                let conn = match self.db.get() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Job worker DB error: {e}");
                        tokio::time::sleep(self.poll_interval).await;
                        continue;
                    }
                };
                queue::claim_batch(&conn, 10)
            };

            // Process pending scheduled sends
            self.clone().process_pending_sends().await;

            if jobs.is_empty() {
                tokio::time::sleep(self.poll_interval).await;
                // Periodic cleanup
                if iteration % 500 == 0 {
                    if let Ok(conn) = self.db.get() {
                        queue::cleanup_completed(&conn, self.cleanup_days);
                    }
                }
                continue;
            }

            // Process each job concurrently (semaphore bounds actual parallelism)
            for job in jobs {
                let worker = self.clone();
                let semaphore = self.semaphore.clone();
                tokio::spawn(async move {
                    let _permit = match semaphore.acquire().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };
                    worker.process_job(job).await;
                });
            }

            // Tight loop when there are jobs to process
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn process_job(&self, job: Job) {
        let result = match job.job_type.as_str() {
            "ai_classify" => self.handle_ai_classify(&job).await,
            "memories_store" => self.handle_memories_store(&job).await,
            "chat_summarize" => self.handle_chat_summarize(&job).await,
            "pref_extract" => self.handle_pref_extract(&job).await,
            _ => Err(format!("Unknown job type: {}", job.job_type)),
        };

        match result {
            Ok(()) => {
                if let Ok(conn) = self.db.get() {
                    queue::complete_job(&conn, job.id, &job.job_type, job.message_id.as_deref());
                }
                self.ws_hub.broadcast(WsEvent::JobCompleted {
                    message_id: job.message_id.clone(),
                    job_type: job.job_type.clone(),
                });
                // Also broadcast AiProcessed for UI updates on classify completion
                if job.job_type == "ai_classify" {
                    if let Some(ref msg_id) = job.message_id {
                        self.ws_hub.broadcast(WsEvent::AiProcessed {
                            message_id: msg_id.clone(),
                        });
                    }
                }
                tracing::debug!(job_id = job.id, job_type = job.job_type, "Job completed");
            }
            Err(e) => {
                tracing::warn!(job_id = job.id, job_type = job.job_type, error = %e, "Job failed");
                if let Ok(conn) = self.db.get() {
                    queue::fail_job(&conn, job.id, &job.job_type, job.message_id.as_deref(), &e, job.attempts + 1, job.max_attempts);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Job handlers
    // -----------------------------------------------------------------------

    async fn handle_ai_classify(&self, job: &Job) -> Result<(), String> {
        let payload: queue::AiClassifyPayload = serde_json::from_str(
            job.payload.as_deref().ok_or("Missing payload")?,
        )
        .map_err(|e| format!("Invalid payload: {e}"))?;

        let message_id = job.message_id.as_deref().ok_or("Missing message_id")?;

        // Check if AI is enabled
        let enabled = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string())
                == "true"
        };

        if !enabled {
            return Err("AI not enabled".to_string());
        }

        if !self.providers.has_providers() {
            return Err("No AI providers configured".to_string());
        }

        // Build feedback context
        let feedback_ctx = self.db.get().ok().and_then(|conn| {
            crate::api::ai_feedback::build_feedback_context(&conn)
        });

        // Load user preferences from Memories
        let prefs = self.memories.search("user email preferences", 1, Some("iris/user/preferences")).await;
        let pref_context = prefs.first().map(|p| {
            format!("\n\nUser preferences:\n{}", p.text)
        });

        // Run AI pipeline
        let metadata = pipeline::process_email(
            &self.providers,
            &payload.subject,
            &payload.from,
            &payload.body,
            feedback_ctx.as_deref(),
            pref_context.as_deref(),
        )
        .await
        .ok_or("AI pipeline returned no result")?;

        // Update message
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let updated = message::update_ai_metadata(
            &conn,
            message_id,
            &metadata.intent,
            metadata.priority_score,
            &metadata.priority_label,
            &metadata.category,
            &metadata.summary,
            metadata.entities.as_deref(),
            metadata.deadline.as_deref(),
        );

        if !updated {
            return Err("Failed to update message metadata".to_string());
        }

        Ok(())
    }

    async fn handle_memories_store(&self, job: &Job) -> Result<(), String> {
        let payload: queue::MemoriesStorePayload = serde_json::from_str(
            job.payload.as_deref().ok_or("Missing payload")?,
        )
        .map_err(|e| format!("Invalid payload: {e}"))?;

        let db_message_id = job.message_id.as_deref().ok_or("Missing message_id")?;

        // Build text content for embedding
        let from = match (&payload.from_name, &payload.from_address) {
            (Some(name), Some(addr)) => format!("From: {} <{}>", name, addr),
            (None, Some(addr)) => format!("From: {}", addr),
            _ => String::new(),
        };
        let subj = payload.subject.as_deref().unwrap_or("(no subject)");
        let date_str = payload
            .date
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();

        let body = payload.body_text.as_deref().unwrap_or("");
        let body_truncated: String = body.chars().take(4000).collect();

        let text = format!(
            "{}\nSubject: {}\nDate: {}\n\n{}",
            from, subj, date_str, body_truncated
        );

        let source = format!("iris/{}/messages/{}", payload.account_id, db_message_id);
        let key = payload
            .rfc_message_id
            .clone()
            .unwrap_or_else(|| db_message_id.to_string());

        if !self.memories.upsert(&text, &source, &key).await {
            return Err("Memories upsert failed".to_string());
        }

        Ok(())
    }

    async fn handle_chat_summarize(&self, job: &Job) -> Result<(), String> {
        let payload: queue::ChatSummarizePayload = serde_json::from_str(
            job.payload.as_deref().ok_or("Missing payload")?,
        )
        .map_err(|e| format!("Invalid payload: {e}"))?;

        // Load session messages
        let messages = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT role, content FROM chat_messages WHERE session_id = ?1 ORDER BY created_at ASC LIMIT 50",
                )
                .map_err(|e| e.to_string())?;
            let msgs: Vec<String> = stmt
                .query_map(rusqlite::params![payload.session_id], |row| {
                    let role: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    Ok(format!("{}: {}", role, content))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            msgs
        };

        if messages.is_empty() {
            return Ok(());
        }

        let conversation = messages.join("\n");

        let prompt = format!(
            "Summarize this email assistant conversation in 2-3 sentences, capturing key topics discussed, actions taken, and user preferences revealed:\n\n{}",
            conversation
        );

        let summary = self
            .providers
            .generate(&prompt, Some("You are a conversation summarizer. Be concise."))
            .await
            .ok_or("AI summarization failed")?;

        // Store summary in Memories
        let source = format!("iris/chat/sessions/{}", payload.session_id);
        if !self.memories.upsert(&summary, &source, &payload.session_id).await {
            return Err("Failed to store chat summary".to_string());
        }

        tracing::info!(session_id = payload.session_id, "Chat session summarized");
        Ok(())
    }

    async fn handle_pref_extract(&self, _job: &Job) -> Result<(), String> {
        // Load correction patterns
        let patterns = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT field, original_value, corrected_value, COUNT(*) as cnt
                     FROM ai_feedback
                     GROUP BY field, original_value, corrected_value
                     ORDER BY cnt DESC
                     LIMIT 20",
                )
                .map_err(|e| e.to_string())?;
            let rows: Vec<String> = stmt
                .query_map([], |row| {
                    let field: String = row.get(0)?;
                    let original: Option<String> = row.get(1)?;
                    let corrected: String = row.get(2)?;
                    let count: i64 = row.get(3)?;
                    let orig = original.unwrap_or_else(|| "unset".to_string());
                    Ok(format!("- {} from \"{}\" to \"{}\" ({} times)", field, orig, corrected, count))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            rows
        };

        if patterns.is_empty() {
            return Ok(());
        }

        let prompt = format!(
            "Based on these email classification correction patterns from the user, generate a concise preference profile (3-5 bullet points) describing how this user prefers emails to be classified:\n\n{}",
            patterns.join("\n")
        );

        let preferences = self
            .providers
            .generate(&prompt, Some("You generate concise user preference profiles for email classification."))
            .await
            .ok_or("AI preference extraction failed")?;

        // Store preferences in Memories
        if !self
            .memories
            .upsert(&preferences, "iris/user/preferences", "preferences")
            .await
        {
            return Err("Failed to store preferences".to_string());
        }

        tracing::info!("User preferences extracted and stored");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Scheduled send processing
    // -----------------------------------------------------------------------

    /// Check for pending_sends that are due (send_at <= now) and send them.
    async fn process_pending_sends(self: Arc<Self>) {
        #[derive(Debug)]
        struct PendingSend {
            id: String,
            account_id: String,
            to_addresses: String,
            cc_addresses: Option<String>,
            bcc_addresses: Option<String>,
            subject: String,
            body_text: String,
            body_html: Option<String>,
            in_reply_to: Option<String>,
            references_header: Option<String>,
        }

        let due_sends: Vec<PendingSend> = {
            let conn = match self.db.get() {
                Ok(c) => c,
                Err(_) => return,
            };
            let now = chrono::Utc::now().timestamp();

            // Claim pending sends that are due: atomically set status to 'sending'
            conn.execute(
                "UPDATE pending_sends SET status = 'sending' WHERE status = 'pending' AND send_at <= ?1",
                rusqlite::params![now],
            ).ok();

            let mut stmt = match conn.prepare(
                "SELECT id, account_id, to_addresses, cc_addresses, bcc_addresses, subject, body_text, body_html, in_reply_to, references_header
                 FROM pending_sends WHERE status = 'sending'"
            ) {
                Ok(s) => s,
                Err(_) => return,
            };

            stmt.query_map([], |row| {
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
        };

        for ps in due_sends {
            let worker = Arc::clone(&self);
            let semaphore = self.semaphore.clone();
            let ps_id = ps.id.clone();
            tokio::spawn(async move {
                let _permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let to: Vec<String> = serde_json::from_str(&ps.to_addresses).unwrap_or_default();
                let cc: Vec<String> = ps.cc_addresses.as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();
                let bcc: Vec<String> = ps.bcc_addresses.as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default();

                let result = worker.execute_scheduled_send(
                    &ps.account_id,
                    &to, &cc, &bcc,
                    &ps.subject,
                    &ps.body_text,
                    ps.body_html.as_deref(),
                    ps.in_reply_to.as_deref(),
                    ps.references_header.as_deref(),
                ).await;

                if let Ok(conn) = worker.db.get() {
                    match result {
                        Ok(()) => {
                            conn.execute(
                                "UPDATE pending_sends SET status = 'sent' WHERE id = ?1",
                                rusqlite::params![ps_id],
                            ).ok();
                            tracing::info!(id = %ps_id, "Scheduled send completed");
                        }
                        Err(e) => {
                            conn.execute(
                                "UPDATE pending_sends SET status = 'failed', error = ?1 WHERE id = ?2",
                                rusqlite::params![e, ps_id],
                            ).ok();
                            tracing::error!(id = %ps_id, error = %e, "Scheduled send failed");
                        }
                    }
                }
            });
        }
    }

    /// Actually send a scheduled email via SMTP.
    async fn execute_scheduled_send(
        &self,
        account_id: &str,
        to: &[String],
        cc: &[String],
        bcc: &[String],
        subject: &str,
        body_text: &str,
        body_html: Option<&str>,
        in_reply_to: Option<&str>,
        references: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let account = Account::get_by_id(&conn, account_id)
            .ok_or_else(|| "Account not found".to_string())?;
        drop(conn);

        if !account.is_active {
            return Err("Account is inactive".to_string());
        }

        let access_token = ensure_fresh_token(&self.db, &account, &self.config)
            .await
            .map_err(|e| format!("Token refresh failed: {e}"))?;

        let compose_req = ComposeRequest {
            account_id: account_id.to_string(),
            to: to.to_vec(),
            cc: cc.to_vec(),
            bcc: bcc.to_vec(),
            subject: subject.to_string(),
            body_text: body_text.to_string(),
            body_html: body_html.map(|s| s.to_string()),
            in_reply_to: in_reply_to.map(|s| s.to_string()),
            references: references.map(|s| s.to_string()),
            schedule_at: None,
        };

        let email = smtp::build_email(
            &account.email,
            account.display_name.as_deref(),
            &compose_req,
        ).map_err(|e| e.to_string())?;

        let rfc_message_id = email
            .headers()
            .get_raw("Message-ID")
            .map(|v| v.to_string());

        smtp::send_email(&account, access_token.as_deref(), email)
            .await
            .map_err(|e| e.to_string())?;

        // Store in Sent folder
        let conn = self.db.get().map_err(|e| e.to_string())?;
        let to_json = serde_json::to_string(to).ok();
        let cc_json = if cc.is_empty() { None } else { serde_json::to_string(cc).ok() };
        let bcc_json = if bcc.is_empty() { None } else { serde_json::to_string(bcc).ok() };

        let sent_msg = InsertMessage {
            account_id: account_id.to_string(),
            message_id: rfc_message_id,
            thread_id: in_reply_to.map(|r| r.trim_matches(|c| c == '<' || c == '>').to_string()),
            folder: "Sent".to_string(),
            from_address: Some(account.email.clone()),
            from_name: account.display_name.clone(),
            to_addresses: to_json,
            cc_addresses: cc_json,
            bcc_addresses: bcc_json,
            subject: Some(subject.to_string()),
            date: Some(chrono::Utc::now().timestamp()),
            snippet: Some(body_text.chars().take(200).collect()),
            body_text: Some(body_text.to_string()),
            body_html: body_html.map(|s| s.to_string()),
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

        InsertMessage::insert(&conn, &sent_msg);
        tracing::info!(account = %account.email, to = ?to, subject = %subject, "Scheduled email sent");

        Ok(())
    }
}
