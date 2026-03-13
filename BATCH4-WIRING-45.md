# Batch 4 Wiring — Feature #45 Deadline Extraction

## Route Wiring (lib.rs)

Add these routes inside `protected_api` in `src/lib.rs`:

```rust
// Deadline endpoints
.route("/deadlines", get(api::deadlines::list_deadlines))
.route("/deadlines/{id}/complete", put(api::deadlines::complete_deadline))
.route("/deadlines/{id}", delete(api::deadlines::delete_deadline))
.route("/threads/{id}/deadlines", get(api::deadlines::thread_deadlines))
.route("/ai/extract-deadlines", post(api::deadlines::extract_deadlines))
```

**Note:** The `/threads/{id}/deadlines` route must be added alongside existing thread routes.
Ensure the more specific `/threads/{id}/deadlines` is registered before the catch-all `/threads/{id}`.

## Module Registration (src/api/mod.rs)

Already done:
```rust
pub mod deadlines;
```

## Migration

- File: `migrations/025_deadlines.sql`
- Registered in `src/db/migrations.rs` as `MIGRATION_025`, applied at version 25

## Files Created/Modified

### New Files
- `migrations/025_deadlines.sql` — deadlines table + indexes
- `src/api/deadlines.rs` — 5 endpoints + unit tests
- `web/src/components/thread/DeadlineList.svelte` — thread deadline display
- `web/src/components/inbox/DeadlineWidget.svelte` — inbox upcoming deadlines widget

### Modified Files
- `src/api/mod.rs` — added `pub mod deadlines;`
- `src/db/migrations.rs` — added migration 025 include + run block
- `web/src/lib/api.ts` — added `Deadline` interface + `deadlines` API methods
