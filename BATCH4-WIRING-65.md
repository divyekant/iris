# Batch 4 Wiring — Feature #65 Follow-up Reminders

## Route Registration (lib.rs — protected_api)

Add to the `protected_api` Router in `src/lib.rs`:

```rust
.route("/followups", get(api::followups::list_followups))
.route("/followups/{id}/snooze", put(api::followups::snooze_followup))
.route("/followups/{id}/dismiss", put(api::followups::dismiss_followup))
.route("/followups/{id}/acted", put(api::followups::mark_acted))
.route("/ai/scan-followups", post(api::followups::scan_followups))
```

## Migration Registration (src/db/migrations.rs)

Add after the last migration constant:

```rust
const MIGRATION_027: &str = include_str!("../../migrations/027_followup_reminders.sql");
```

Add to the `run()` function:

```rust
if current_version < 27 {
    conn.execute_batch(MIGRATION_027)?;
    tracing::info!("Applied migration 027_followup_reminders");
}
```

## Job Queue Integration

A new job type `scan_followups` can be added to the `processing_jobs` table's CHECK constraint and to `src/jobs/worker.rs` to run periodic follow-up scans. The job would:

1. Call the same logic as `scan_followups()` handler
2. Run on a daily schedule (or configurable interval)
3. No payload needed — scans all sent emails from last 7 days

To add the job type, update the CHECK constraint in `migrations/005_job_queue.sql` (or create a new migration):

```sql
-- New migration to extend job types
ALTER TABLE processing_jobs DROP CONSTRAINT IF EXISTS processing_jobs_job_type_check;
-- SQLite doesn't support ALTER CONSTRAINT; use a new table or add the type via application logic
```

Since SQLite CHECK constraints cannot be altered in-place, the recommended approach is to handle the `scan_followups` job type at the application level in `src/jobs/worker.rs` by adding a match arm, without modifying the existing CHECK constraint. The worker would call `scan_followups` logic from `src/api/followups.rs`.

## Frontend Integration

Import `FollowupPanel` in the inbox page:

```svelte
<script>
  import FollowupPanel from '../components/inbox/FollowupPanel.svelte';
</script>

<!-- Add above or below the message list -->
<FollowupPanel />
```

## Files Created/Modified

### New Files
- `migrations/027_followup_reminders.sql` — table + indexes
- `src/api/followups.rs` — 5 endpoints + unit tests
- `web/src/components/inbox/FollowupPanel.svelte` — collapsible panel
- `BATCH4-WIRING-65.md` — this file

### Modified Files
- `src/api/mod.rs` — added `pub mod followups;`
- `web/src/lib/api.ts` — added `FollowupReminder` interface + `followups` API methods
