# Feature #43 Intent Detection — Wiring Instructions

## Route additions for `src/lib.rs`

Add these two routes to the `protected_api` Router (inside `build_app`), after the existing `/ai/reprocess` route:

```rust
.route("/messages/{id}/intent", get(api::intent::get_intent))
.route("/ai/detect-intent", post(api::intent::detect_intent))
```

## Module registration (already done)

`pub mod intent;` has been added to `src/api/mod.rs`.

## Migration registration (already done)

Migration 024 has been registered in `src/db/migrations.rs`.

## Job queue integration (optional enhancement)

To run intent detection automatically during email sync, add to `src/jobs/worker.rs`:

1. Add a new match arm in `process_job`:
```rust
"intent_detect" => self.handle_intent_detect(&job).await,
```

2. Add the handler method:
```rust
async fn handle_intent_detect(&self, job: &Job) -> Result<(), String> {
    let payload: queue::AiClassifyPayload = serde_json::from_str(
        job.payload.as_deref().ok_or("Missing payload")?,
    ).map_err(|e| format!("Invalid payload: {e}"))?;

    let message_id = job.message_id.as_deref().ok_or("Missing message_id")?;

    let result = crate::api::intent::detect_intent_for_message(
        &self.providers,
        &payload.subject,
        &payload.from,
        &payload.body,
    ).await.ok_or("Intent detection returned no result")?;

    let conn = self.db.get().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE messages SET intent = ?2, intent_confidence = ?3, updated_at = unixepoch() WHERE id = ?1",
        rusqlite::params![message_id, result.intent, result.confidence],
    ).map_err(|e| e.to_string())?;

    Ok(())
}
```

3. Add `intent_detect` to the `job_type` CHECK constraint in the `processing_jobs` table (requires a new migration or altering the constraint).

4. Enqueue alongside `ai_classify` in `src/jobs/queue.rs`:
```rust
pub fn enqueue_intent_detect(conn: &Connection, message_id: &str, subject: &str, from: &str, body: &str) {
    let payload = serde_json::to_string(&AiClassifyPayload {
        subject: subject.to_string(),
        from: from.to_string(),
        body: body.to_string(),
    }).unwrap_or_default();

    conn.execute(
        "INSERT INTO processing_jobs (job_type, message_id, payload) VALUES ('intent_detect', ?1, ?2)",
        rusqlite::params![message_id, payload],
    ).ok();
}
```

## Files created/modified

### New files
- `migrations/024_intent_detection.sql`
- `src/api/intent.rs`
- `web/src/components/inbox/IntentBadge.svelte`

### Modified files
- `src/api/mod.rs` — added `pub mod intent;`
- `src/db/migrations.rs` — registered migration 024
- `src/models/message.rs` — added `intent` field to `MessageSummary`, updated queries
- `src/api/messages.rs` — added `m.intent` to select columns
- `web/src/lib/api.ts` — added `intent` namespace
- `web/src/components/inbox/MessageRow.svelte` — imported IntentBadge, added to template
