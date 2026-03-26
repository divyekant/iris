use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::ai::memories::{MemoriesClient, SearchOptions};
use crate::ai::provider::ProviderPool;
use crate::ai::pipeline;
use crate::api::compose::{self, PendingSend};
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
        let mut last_decay = std::time::Instant::now();

        loop {
            iteration += 1;

            // --- Process pending sends ---
            self.process_pending_sends().await;

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
                // Periodic cleanup + snooze wake-up
                if iteration % 500 == 0 {
                    if let Ok(conn) = self.db.get() {
                        queue::cleanup_completed(&conn, self.cleanup_days);
                    }
                }
                // Check for snoozed messages to wake up every 15 cycles (~30s at 2s poll)
                if iteration % 15 == 0 {
                    if let Ok(conn) = self.db.get() {
                        let woken = message::wake_snoozed(&conn);
                        if woken > 0 {
                            tracing::info!("Unsnoozed {woken} message(s)");
                            self.ws_hub.broadcast(WsEvent::NewEmail {
                                account_id: String::new(),
                                message_id: String::new(),
                            });
                        }
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

    /// Check for pending sends that are ready and send them via SMTP.
    async fn process_pending_sends(&self) {
        let pending = {
            let conn = match self.db.get() {
                Ok(c) => c,
                Err(_) => return,
            };
            compose::claim_pending_sends(&conn)
        };

        for ps in pending {
            let worker_db = self.db.clone();
            let worker_config = self.config.clone();
            let ws_hub = self.ws_hub.clone();
            let ps_clone = ps;
            // Process each pending send in its own task
            tokio::spawn(async move {
                if let Err(e) = send_pending_email(&worker_db, &worker_config, &ps_clone).await {
                    tracing::error!(pending_send_id = %ps_clone.id, error = %e, "Failed to send pending email");
                    if let Ok(conn) = worker_db.get() {
                        compose::mark_pending_failed(&conn, &ps_clone.id);
                    }
                } else {
                    if let Ok(conn) = worker_db.get() {
                        compose::mark_pending_sent(&conn, &ps_clone.id);
                    }
                    ws_hub.broadcast(WsEvent::JobCompleted {
                        message_id: Some(ps_clone.id.clone()),
                        job_type: "pending_send".to_string(),
                    });
                    tracing::info!(pending_send_id = %ps_clone.id, "Pending email sent successfully");
                }
            });
        }
    }

    async fn process_job(&self, job: Job) {
        let result = match job.job_type.as_str() {
            "ai_classify" => self.handle_ai_classify(&job).await,
            "memories_store" => self.handle_memories_store(&job).await,
            "chat_summarize" => self.handle_chat_summarize(&job).await,
            "pref_extract" => self.handle_pref_extract(&job).await,
            "entity_extract" => self.handle_entity_extract(&job).await,
            "style_extract" => self.handle_style_extract(&job).await,
            "auto_draft" => self.handle_auto_draft(&job).await,
            "delegation_process" => self.handle_delegation_process(&job).await,
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
        let prefs = self.memories.search("user email preferences", 1, Some("iris/user/preferences"), SearchOptions::default()).await;
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
            metadata.sentiment.as_deref(),
            metadata.needs_reply,
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

        // Convert email timestamp to ISO 8601 for temporal search
        let document_at = payload
            .date
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());

        if !self.memories.upsert(&text, &source, &key, document_at.as_deref()).await {
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
        if !self.memories.upsert(&summary, &source, &payload.session_id, None).await {
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
            .upsert(&preferences, "iris/user/preferences", "preferences", None)
            .await
        {
            return Err("Failed to store preferences".to_string());
        }

        tracing::info!("User preferences extracted and stored");
        Ok(())
    }

    async fn handle_entity_extract(&self, job: &Job) -> Result<(), String> {
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

        // Fetch message
        let (subject, from_address, body_text) = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT COALESCE(subject, ''), COALESCE(from_address, ''), COALESCE(body_text, '') FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .map_err(|e| format!("Message not found: {e}"))?
        };

        let body_truncated: String = body_text.chars().take(4000).collect();

        // Build extraction prompt
        let prompt = format!(
            r#"Extract entities, relationships, and date events from this email. Return a JSON object with these fields:

1. "entities": array of {{ "name": string, "type": "person"|"org"|"project"|"date"|"amount", "aliases": [string] }}
2. "relations": array of {{ "from": string, "to": string, "relation": string }}
3. "events": array of {{ "name": string, "date": "YYYY-MM-DD", "precision": "day"|"week"|"month"|"quarter"|"year" }}

Return empty arrays if nothing is found. Respond ONLY with the JSON object.

From: {from_address}
Subject: {subject}

Body:
{body_truncated}"#
        );

        let response = self
            .providers
            .generate(&prompt, Some("You are an entity and relationship extraction engine for emails. Return ONLY valid JSON."))
            .await
            .ok_or("AI entity extraction failed")?;

        let trimmed = response.trim();
        let json_str = if trimmed.starts_with("```") {
            trimmed
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
        } else {
            trimmed
        };

        // Parse and store (best-effort)
        #[derive(serde::Deserialize)]
        struct AiEntity { name: String, #[serde(rename = "type")] entity_type: String, #[serde(default)] aliases: Vec<String> }
        #[derive(serde::Deserialize)]
        struct AiRelation { from: String, to: String, relation: String }
        #[derive(serde::Deserialize)]
        struct AiEvent { name: String, date: String, #[serde(default = "default_prec")] precision: String }
        fn default_prec() -> String { "day".to_string() }
        #[derive(serde::Deserialize)]
        struct AiResult { #[serde(default)] entities: Vec<AiEntity>, #[serde(default)] relations: Vec<AiRelation>, #[serde(default)] events: Vec<AiEvent> }

        let extraction: AiResult = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse AI response: {e}"))?;

        let conn = self.db.get().map_err(|e| e.to_string())?;
        let valid_types = ["person", "org", "project", "date", "amount"];
        let valid_precisions = ["day", "week", "month", "quarter", "year"];

        let mut name_to_id = std::collections::HashMap::new();

        for e in &extraction.entities {
            let et = e.entity_type.to_lowercase();
            if !valid_types.contains(&et.as_str()) { continue; }

            let entity_id = crate::api::knowledge_graph::find_or_create_entity(&conn, &e.name, &et, message_id);
            if let Some(ref id) = entity_id {
                name_to_id.insert(e.name.to_lowercase(), id.clone());
                for alias in &e.aliases {
                    if alias.is_empty() { continue; }
                    let exists: bool = conn
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM entity_aliases WHERE entity_id = ?1 AND alias_name = ?2 COLLATE NOCASE)",
                            rusqlite::params![id, alias],
                            |row| row.get(0),
                        )
                        .unwrap_or(true);
                    if !exists {
                        let alias_id = uuid::Uuid::new_v4().to_string();
                        conn.execute(
                            "INSERT INTO entity_aliases (id, entity_id, alias_name, source_message_id) VALUES (?1, ?2, ?3, ?4)",
                            rusqlite::params![alias_id, id, alias, message_id],
                        ).ok();
                    }
                }
            }
        }

        for r in &extraction.relations {
            let a_id = name_to_id.get(&r.from.to_lowercase());
            let b_id = name_to_id.get(&r.to.to_lowercase());
            if let (Some(a), Some(b)) = (a_id, b_id) {
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM entity_relations WHERE entity_a = ?1 AND entity_b = ?2 AND relation_type = ?3)",
                        rusqlite::params![a, b, r.relation],
                        |row| row.get(0),
                    )
                    .unwrap_or(true);
                if exists {
                    conn.execute(
                        "UPDATE entity_relations SET weight = weight + 0.5 WHERE entity_a = ?1 AND entity_b = ?2 AND relation_type = ?3",
                        rusqlite::params![a, b, r.relation],
                    ).ok();
                } else {
                    let rel_id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO entity_relations (id, entity_a, entity_b, relation_type, source_message_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![rel_id, a, b, r.relation, message_id],
                    ).ok();
                }
            }
        }

        for ev in &extraction.events {
            if ev.name.is_empty() || ev.date.is_empty() { continue; }
            let precision = {
                let p = ev.precision.to_lowercase();
                if valid_precisions.contains(&p.as_str()) { p } else { "day".to_string() }
            };
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM timeline_events WHERE event_name = ?1 COLLATE NOCASE AND approximate_date = ?2)",
                    rusqlite::params![ev.name, ev.date],
                    |row| row.get(0),
                )
                .unwrap_or(true);
            if !exists {
                let event_id = uuid::Uuid::new_v4().to_string();
                let account_id: Option<String> = conn
                    .query_row("SELECT account_id FROM messages WHERE id = ?1", rusqlite::params![message_id], |row| row.get(0))
                    .ok();
                conn.execute(
                    "INSERT INTO timeline_events (id, event_name, approximate_date, date_precision, source_message_id, account_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![event_id, ev.name, ev.date, precision, message_id, account_id],
                ).ok();
            }
        }

        tracing::info!(message_id = message_id, entities = extraction.entities.len(), relations = extraction.relations.len(), events = extraction.events.len(), "Entity extraction completed");
        Ok(())
    }

    async fn handle_style_extract(&self, job: &Job) -> Result<(), String> {
        let payload_str = job.payload.as_deref().ok_or("Missing payload")?;
        let payload: serde_json::Value = serde_json::from_str(payload_str)
            .map_err(|e| format!("Invalid payload: {e}"))?;
        let account_id = payload["account_id"]
            .as_str()
            .ok_or("Missing account_id in payload")?;

        // Check if AI is enabled
        let enabled = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string())
                == "true"
        };

        if !enabled || !self.providers.has_providers() {
            return Err("AI not enabled".to_string());
        }

        // Fetch sent emails
        let sent_emails: Vec<(String, String)> = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT COALESCE(subject, ''), COALESCE(body_text, '')
                     FROM messages
                     WHERE account_id = ?1 AND folder = 'Sent'
                     ORDER BY date DESC
                     LIMIT 200",
                )
                .map_err(|e| e.to_string())?;

            stmt.query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .filter(|(_, body)| !body.trim().is_empty())
            .collect()
        };

        if sent_emails.is_empty() {
            tracing::info!(account_id = account_id, "No sent emails for style extraction");
            return Ok(());
        }

        // Build sample
        let mut sample = String::new();
        let max_sample = 6000;
        for (i, (subject, body)) in sent_emails.iter().enumerate().take(30) {
            let body_truncated: String = body.chars().take(300).collect();
            let entry = format!("--- Email {} ---\nSubject: {}\n{}\n\n", i + 1, subject, body_truncated);
            if sample.len() + entry.len() > max_sample {
                break;
            }
            sample.push_str(&entry);
        }

        let prompt = format!(
            r#"Analyze the writing style of these sent emails and extract the following traits. Return ONLY a JSON array:

[
  {{"trait_type": "greeting", "trait_value": "most common greeting phrase", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "signoff", "trait_value": "most common sign-off phrase", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "tone", "trait_value": "one of: formal, semi-formal, casual, direct, warm, analytical", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "avg_length", "trait_value": "short | medium | long", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "formality", "trait_value": "1-10 score", "confidence": 0.0-1.0, "examples": []}},
  {{"trait_type": "vocabulary", "trait_value": "description", "confidence": 0.0-1.0, "examples": []}}
]

Emails:

{sample}"#
        );

        let raw = self
            .providers
            .generate(&prompt, Some("You are a writing style analyst. Return ONLY valid JSON."))
            .await
            .ok_or("AI style analysis failed")?;

        // Parse and store
        let traits = crate::api::writing_style::parse_style_traits_for_worker(&raw, account_id);
        let conn = self.db.get().map_err(|e| e.to_string())?;
        crate::api::writing_style::store_style_traits_for_worker(&conn, account_id, &traits);

        // Record timestamp
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            rusqlite::params![
                format!("style_last_analyzed_{}", account_id),
                chrono::Utc::now().timestamp().to_string()
            ],
        )
        .ok();

        tracing::info!(account_id = account_id, traits = traits.len(), "Writing style extracted");
        Ok(())
    }

    async fn handle_auto_draft(&self, job: &Job) -> Result<(), String> {
        let message_id = job.message_id.as_deref().ok_or("Missing message_id")?;

        // Delegation precedence: if any delegation playbook matched, skip auto_draft
        {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            if crate::api::delegation::has_delegation_match(&conn, message_id) {
                tracing::debug!(message_id = message_id, "Skipping auto_draft: delegation playbook matched");
                return Ok(());
            }
        }

        // Check if AI is enabled
        let enabled = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row("SELECT value FROM config WHERE key = 'ai_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string())
                == "true"
        };

        if !enabled || !self.providers.has_providers() {
            return Err("AI not enabled".to_string());
        }

        // Check auto-draft is still enabled
        let ad_enabled = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row("SELECT value FROM config WHERE key = 'auto_draft_enabled'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "false".to_string())
                == "true"
        };

        if !ad_enabled {
            return Ok(());
        }

        // Fetch the message
        let (account_id, from_address, subject, body_text) = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT account_id, COALESCE(from_address, ''), COALESCE(subject, ''), COALESCE(body_text, '')
                 FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)),
            )
            .map_err(|e| format!("Message not found: {e}"))?
        };

        // Skip if already has a draft
        let existing: bool = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM auto_drafts WHERE message_id = ?1)",
                rusqlite::params![message_id],
                |row| row.get(0),
            )
            .unwrap_or(false)
        };

        if existing {
            return Ok(());
        }

        // Get sensitivity
        let confidence_threshold = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            let sensitivity = conn
                .query_row("SELECT value FROM config WHERE key = 'auto_draft_sensitivity'", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "balanced".to_string());
            match sensitivity.as_str() {
                "conservative" => 0.8,
                "aggressive" => 0.4,
                _ => 0.6,
            }
        };

        // Try pattern matching
        let sender_domain = from_address.split('@').nth(1).unwrap_or("").to_lowercase();
        let matched_pattern = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            crate::api::auto_draft::find_matching_pattern_for_worker(&conn, &account_id, &sender_domain, &subject, confidence_threshold)
        };

        let (draft_body, pattern_id) = if let Some((pid, template)) = matched_pattern {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE auto_draft_patterns SET match_count = match_count + 1, last_matched_at = unixepoch(), updated_at = unixepoch() WHERE id = ?1",
                rusqlite::params![pid],
            ).ok();
            (template, Some(pid))
        } else {
            // Generate via AI
            let body_truncated: String = body_text.chars().take(2000).collect();

            let style_snippet = {
                let conn = self.db.get().map_err(|e| e.to_string())?;
                crate::api::writing_style::build_style_prompt(&conn, &account_id)
            };

            let mut system = String::from(
                "You are an email reply assistant. Generate a professional, contextual reply. \
                 Return ONLY the reply body text."
            );
            if let Some(style) = style_snippet {
                system.push_str("\n\n");
                system.push_str(&style);
            }

            let prompt = format!(
                "Write a reply to this email:\n\nFrom: {}\nSubject: {}\n\n{}",
                from_address, subject, body_truncated
            );

            let generated = self.providers
                .generate(&prompt, Some(&system))
                .await
                .ok_or("AI draft generation failed")?;

            (generated, None)
        };

        // Store the draft
        let draft_id = uuid::Uuid::new_v4().to_string();
        let conn = self.db.get().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO auto_drafts (id, message_id, account_id, pattern_id, draft_body, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
            rusqlite::params![draft_id, message_id, account_id, pattern_id, draft_body],
        )
        .map_err(|e| format!("Failed to store auto-draft: {e}"))?;

        tracing::info!(message_id = message_id, "Auto-draft generated");
        Ok(())
    }

    async fn handle_delegation_process(&self, job: &Job) -> Result<(), String> {
        let message_id = job.message_id.as_deref().ok_or("Missing message_id")?;

        // Fetch message details
        let (account_id, from_address, subject, category, intent) = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            conn.query_row(
                "SELECT account_id, COALESCE(from_address, ''), COALESCE(subject, ''), ai_category, intent FROM messages WHERE id = ?1",
                rusqlite::params![message_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?)),
            )
            .map_err(|e| format!("Message not found: {e}"))?
        };

        let sender_domain = from_address.split('@').nth(1).unwrap_or("").to_lowercase();
        let subject_lower = subject.to_lowercase();

        // Get all enabled playbooks for this account
        let playbooks: Vec<(String, String, String, Option<String>, f64)> = {
            let conn = self.db.get().map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, trigger_conditions, action_type, action_template, confidence_threshold
                     FROM delegation_playbooks
                     WHERE account_id = ?1 AND enabled = 1",
                )
                .map_err(|e| e.to_string())?;

            stmt.query_map(rusqlite::params![account_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
        };

        let mut any_match = false;

        for (pb_id, trigger_json, action_type, action_template, threshold) in playbooks {
            let conditions: crate::api::delegation::TriggerConditions =
                match serde_json::from_str(&trigger_json) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

            let mut matched = 0u32;
            let mut total = 0u32;

            if let Some(ref domain) = conditions.sender_domain {
                total += 1;
                if sender_domain == domain.to_lowercase() { matched += 1; }
            }
            if let Some(ref substr) = conditions.subject_contains {
                total += 1;
                if subject_lower.contains(&substr.to_lowercase()) { matched += 1; }
            }
            if let Some(ref cat) = conditions.category {
                total += 1;
                if category.as_deref().unwrap_or("").eq_ignore_ascii_case(cat) { matched += 1; }
            }
            if let Some(ref int) = conditions.intent {
                total += 1;
                if intent.as_deref().unwrap_or("").eq_ignore_ascii_case(int) { matched += 1; }
            }

            if total == 0 { continue; }
            let confidence = matched as f64 / total as f64;

            let conn = self.db.get().map_err(|e| e.to_string())?;

            if confidence >= threshold {
                any_match = true;
                let action_id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO delegation_actions (id, playbook_id, message_id, action_taken, confidence, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'completed')",
                    rusqlite::params![action_id, pb_id, message_id, action_type, confidence],
                ).ok();
                conn.execute(
                    "UPDATE delegation_playbooks SET match_count = match_count + 1, last_matched_at = unixepoch(), updated_at = unixepoch() WHERE id = ?1",
                    rusqlite::params![pb_id],
                ).ok();
                // Execute the action
                crate::api::delegation::execute_delegation_action_pub(&conn, message_id, &action_type, action_template.as_deref());
                tracing::info!(message_id = message_id, playbook = %pb_id, action = %action_type, "Delegation action executed");
            } else if confidence > 0.5 {
                any_match = true;
                let action_id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO delegation_actions (id, playbook_id, message_id, action_taken, confidence, status)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'pending_review')",
                    rusqlite::params![action_id, pb_id, message_id, action_type, confidence],
                ).ok();
                tracing::info!(message_id = message_id, playbook = %pb_id, "Delegation action pending review");
            }
        }

        if !any_match {
            tracing::debug!(message_id = message_id, "No delegation playbooks matched");
        }

        Ok(())
    }
}

/// Actually send a pending email via SMTP and store in Sent folder.
async fn send_pending_email(
    db: &DbPool,
    config: &Config,
    ps: &PendingSend,
) -> Result<(), String> {
    let conn = db.get().map_err(|e| e.to_string())?;
    let account = Account::get_by_id(&conn, &ps.account_id)
        .ok_or_else(|| format!("Account {} not found", ps.account_id))?;
    drop(conn);

    if !account.is_active {
        return Err("Account is inactive".to_string());
    }

    // Refresh OAuth token
    let access_token = ensure_fresh_token(db, &account, config)
        .await
        .map_err(|e| format!("Token refresh failed: {e}"))?;

    // Parse addresses from JSON
    let to: Vec<String> = serde_json::from_str(&ps.to_addresses)
        .map_err(|e| format!("Invalid to_addresses JSON: {e}"))?;
    let cc: Vec<String> = ps
        .cc_addresses
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let bcc: Vec<String> = ps
        .bcc_addresses
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    let compose_req = ComposeRequest {
        account_id: ps.account_id.clone(),
        to: to.clone(),
        cc: cc.clone(),
        bcc: bcc.clone(),
        subject: ps.subject.clone().unwrap_or_default(),
        body_text: ps.body_text.clone().unwrap_or_default(),
        body_html: ps.body_html.clone(),
        in_reply_to: ps.in_reply_to.clone(),
        references: ps.references_header.clone(),
        attachments: Vec::new(),
        schedule_at: None,
    };

    // Build the email
    let email = smtp::build_email(
        &account.email,
        account.display_name.as_deref(),
        &compose_req,
    )
    .map_err(|e| e.to_string())?;

    // Extract the Message-ID
    let rfc_message_id = email
        .headers()
        .get_raw("Message-ID")
        .map(|v| v.to_string());

    // Send via SMTP
    smtp::send_email(&account, access_token.as_deref(), email)
        .await
        .map_err(|e| e.to_string())?;

    // Store in Sent folder
    let to_json = serde_json::to_string(&to).ok();
    let cc_json = if cc.is_empty() { None } else { serde_json::to_string(&cc).ok() };
    let bcc_json = if bcc.is_empty() { None } else { serde_json::to_string(&bcc).ok() };

    let sent_msg = InsertMessage {
        account_id: ps.account_id.clone(),
        message_id: rfc_message_id,
        thread_id: ps.in_reply_to.as_ref().map(|r| r.trim_matches(|c| c == '<' || c == '>').to_string()),
        folder: "Sent".to_string(),
        from_address: Some(account.email.clone()),
        from_name: account.display_name.clone(),
        to_addresses: to_json,
        cc_addresses: cc_json,
        bcc_addresses: bcc_json,
        subject: ps.subject.clone(),
        date: Some(chrono::Utc::now().timestamp()),
        snippet: ps.body_text.as_ref().map(|t| t.chars().take(200).collect()),
        body_text: ps.body_text.clone(),
        body_html: ps.body_html.clone(),
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
        list_unsubscribe: None,
        list_unsubscribe_post: false,
    };

    let conn = db.get().map_err(|e| e.to_string())?;
    InsertMessage::insert(&conn, &sent_msg).ok_or("Failed to store sent message")?;

    tracing::info!(account = %account.email, to = ?to, subject = ?ps.subject, "Email sent (from pending queue)");
    Ok(())
}
