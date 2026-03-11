use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::ai::memories::MemoriesClient;
use crate::ai::provider::ProviderPool;
use crate::ai::pipeline;
use crate::db::DbPool;
use crate::jobs::queue::{self, Job};
use crate::models::message;
use crate::ws::hub::{WsEvent, WsHub};

/// Background worker that polls the job queue and processes jobs.
pub struct JobWorker {
    db: DbPool,
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
        ws_hub: WsHub,
        providers: ProviderPool,
        memories: MemoriesClient,
        poll_interval_ms: u64,
        max_concurrency: usize,
        cleanup_days: i64,
    ) -> Self {
        Self {
            db,
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
        let mut last_decay = std::time::Instant::now();

        loop {
            iteration += 1;

            // Periodic priority decay (every hour)
            if last_decay.elapsed() > std::time::Duration::from_secs(3600) {
                self.run_priority_decay();
                last_decay = std::time::Instant::now();
            }

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
    // Periodic tasks
    // -----------------------------------------------------------------------

    /// Run priority decay on stale messages, reading config from the DB.
    fn run_priority_decay(&self) {
        let conn = match self.db.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Priority decay: DB error: {e}");
                return;
            }
        };

        // Read decay config from DB (defaults: enabled=true, threshold=7, factor=0.85)
        let enabled = conn
            .query_row("SELECT value FROM config WHERE key = 'decay_enabled'", [], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "true".to_string())
            == "true";

        if !enabled {
            return;
        }

        let threshold_days: i64 = conn
            .query_row("SELECT value FROM config WHERE key = 'decay_threshold_days'", [], |row| row.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);

        let decay_factor: f64 = conn
            .query_row("SELECT value FROM config WHERE key = 'decay_factor'", [], |row| row.get::<_, String>(0))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.85);

        let decayed = message::decay_priority_scores(&conn, threshold_days, decay_factor);
        if decayed > 0 {
            tracing::info!("Decayed priority for {} messages (threshold={}d, factor={})", decayed, threshold_days, decay_factor);
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
}
