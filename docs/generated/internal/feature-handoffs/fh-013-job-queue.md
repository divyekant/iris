---
id: fh-013
title: "Feature Handoff: Job Queue & Long-Term Memory"
feature: job-queue
audience: internal
generated: 2026-03-04
status: current
source-tier: direct
hermes-version: 1.0.0
---

# Feature Handoff: Job Queue & Long-Term Memory

## What It Does

The job queue system replaces fire-and-forget `tokio::spawn` calls with a persistent, SQLite-backed work queue for all async processing tasks. Jobs are tracked with status, retry counts, and error details. A background worker polls the queue and processes jobs with semaphore-limited concurrency and exponential backoff retry on failure.

This also introduces long-term memory features: chat session summaries are stored in Memories for cross-session context, and user classification preferences are extracted from AI feedback corrections and injected into future classification prompts.

## Architecture Overview

```
Email Sync ──enqueue──> processing_jobs table
AI Feedback ──enqueue──>        |
Chat (every 10 msgs) ──enqueue──>   |
                                |
                          JobWorker (polling loop)
                                |
                ┌───────────────┼───────────────┐
                v               v               v
         ai_classify     memories_store    chat_summarize
                                                |
                                          pref_extract
                                                |
                        ┌───────────────────────┘
                        v
              Ollama + Memories (vector store)
```

The worker runs as a single long-lived Tokio task spawned at server startup. It polls the `processing_jobs` table at a configurable interval, claims batches of pending jobs, and processes them concurrently up to a semaphore limit.

## Database Schema

### processing_jobs table (migration 005)

Source: `migrations/005_job_queue.sql`

```sql
CREATE TABLE IF NOT EXISTS processing_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL CHECK(job_type IN ('ai_classify','memories_store','chat_summarize','pref_extract')),
    message_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','processing','done','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 4,
    payload TEXT,
    error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_retry_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_jobs_poll ON processing_jobs(status, next_retry_at) WHERE status IN ('pending','processing');
CREATE INDEX idx_jobs_message ON processing_jobs(message_id, job_type);
```

### Status columns on messages table

Migration 005 also adds two columns to the `messages` table:

| Column | Type | Default | Description |
|---|---|---|---|
| `ai_status` | TEXT | NULL | Tracks AI classification job state: `pending`, `done`, `failed` |
| `memories_status` | TEXT | NULL | Tracks Memories storage job state: `pending`, `done`, `failed` |

These columns are updated by the queue functions when jobs are enqueued, completed, or permanently failed.

## Job Types and Payloads

### ai_classify

Runs the AI classification pipeline on a single email message.

- **message_id**: set to the message's database ID
- **Payload** (`AiClassifyPayload`):
  ```json
  { "subject": "...", "from": "...", "body": "..." }
  ```
- **Enqueued by**: `SyncEngine::initial_sync` after inserting each message
- **Processing**: Checks `ai_enabled` and `ai_model` config. Loads feedback context from `ai_feedback` table. Loads user preferences from Memories (`iris/user/preferences`). Calls `pipeline::process_email` with both contexts. Updates message AI metadata columns on success.
- **On completion**: Sets `messages.ai_status = 'done'`. Broadcasts `WsEvent::JobCompleted` and `WsEvent::AiProcessed` via WebSocket.

### memories_store

Stores an email's content in the Memories vector store for semantic search.

- **message_id**: set to the message's database ID
- **Payload** (`MemoriesStorePayload`):
  ```json
  {
    "account_id": "...",
    "rfc_message_id": "<msg@example.com>",
    "from_name": "Alice",
    "from_address": "alice@example.com",
    "subject": "...",
    "body_text": "...",
    "date": 1709510400
  }
  ```
- **Enqueued by**: `SyncEngine::initial_sync` after inserting each message
- **Processing**: Builds a text representation (`From: ... Subject: ... Date: ... Body: ...`), truncates body to 4000 chars, upserts to Memories with source `iris/{account_id}/messages/{db_message_id}`.
- **On completion**: Sets `messages.memories_status = 'done'`. Broadcasts `WsEvent::JobCompleted`.

### chat_summarize

Summarizes a chat session and stores the summary in Memories for cross-session context.

- **message_id**: not set (NULL)
- **Payload** (`ChatSummarizePayload`):
  ```json
  { "session_id": "..." }
  ```
- **Enqueued by**: `POST /api/ai/chat` handler every 10 messages in a session (`msg_count % 10 == 0`)
- **Deduplication**: Before inserting, checks if a `pending` or `processing` `chat_summarize` job already exists for the same session_id. Skips if one exists.
- **Processing**: Loads up to 50 messages from `chat_messages` for the session. Sends conversation text to Ollama with a summarization prompt. Stores the summary in Memories with source `iris/chat/sessions/{session_id}`.
- **On completion**: Broadcasts `WsEvent::JobCompleted`.

### pref_extract

Extracts user classification preferences from accumulated AI feedback corrections and stores them in Memories.

- **message_id**: not set (NULL)
- **Payload**: none (NULL)
- **Enqueued by**: `PUT /api/messages/{id}/ai-feedback` handler every 10 total corrections (`total % 10 == 0`)
- **Deduplication**: Before inserting, checks if a `pending` or `processing` `pref_extract` job already exists. Skips if one exists.
- **Processing**: Loads top 20 correction patterns from `ai_feedback` (grouped by field/original/corrected, ordered by count DESC). Sends patterns to Ollama with a prompt requesting a 3-5 bullet point preference profile. Stores the result in Memories with source `iris/user/preferences` and key `preferences`.
- **On completion**: Broadcasts `WsEvent::JobCompleted`.

## Worker Behavior

Source: `src/jobs/worker.rs`

### Polling Loop

1. The worker calls `claim_batch(&conn, 10)` to atomically SELECT and UPDATE up to 10 pending jobs to `processing` status.
2. `claim_batch` only selects jobs where `status = 'pending'`, `next_retry_at <= now`, and `attempts < max_attempts`.
3. If no jobs are found, the worker sleeps for `poll_interval` (default 2000ms) and loops.
4. If jobs are found, each job is spawned as a Tokio task gated by a semaphore (default 4 permits).
5. After dispatching a batch, the worker sleeps 100ms before polling again (tight loop when work is available).

### Concurrency

Concurrency is controlled by a `tokio::sync::Semaphore` initialized with `job_max_concurrency` permits (default 4). Each spawned job task acquires a permit before processing and releases it when done. This prevents overwhelming Ollama or Memories with too many parallel requests.

### Retry Logic

When a job fails:

1. If `attempts >= max_attempts` (default 4): the job is marked `status = 'failed'`, the error message is stored, and the corresponding `messages` column (`ai_status` or `memories_status`) is set to `'failed'`.
2. If `attempts < max_attempts`: the job is set back to `status = 'pending'` with `next_retry_at` calculated as exponential backoff:
   - Formula: `next_retry_at = now + (attempts^2 * 5)` seconds
   - Attempt 1 failure: retry after 5 seconds
   - Attempt 2 failure: retry after 20 seconds
   - Attempt 3 failure: retry after 45 seconds
   - Attempt 4 (final): permanent failure

### Cleanup

Every 500 polling iterations (when the queue is empty), the worker calls `cleanup_completed(&conn, job_cleanup_days)` to delete `done` jobs older than the configured number of days (default 7). This prevents unbounded table growth.

### WebSocket Notifications

On job completion, the worker broadcasts `WsEvent::JobCompleted { message_id, job_type }`. For `ai_classify` jobs specifically, it also broadcasts `WsEvent::AiProcessed { message_id }` so the frontend can refresh priority badges and category pills without a full page reload.

## Configuration

| Config Key | Default | Description |
|---|---|---|
| `job_poll_interval_ms` | 2000 | How often the worker polls for new jobs (milliseconds) |
| `job_max_concurrency` | 4 | Maximum concurrent job processing tasks (semaphore permits) |
| `job_cleanup_days` | 7 | Delete completed jobs older than this many days |

Environment variables (inherited from existing config):

| Variable | Default | Description |
|---|---|---|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint (used by ai_classify, chat_summarize, pref_extract) |
| `MEMORIES_URL` | `http://localhost:8900` | Memories server endpoint (used by memories_store, chat_summarize, pref_extract) |

## Queue Status API

### GET /api/ai/queue-status

Source: `src/api/queue_status.rs`

Returns current job queue statistics.

**Response:**

```json
{
  "pending": 12,
  "processing": 3,
  "failed": 1,
  "done_today": 247
}
```

| Field | Description |
|---|---|
| `pending` | Jobs waiting to be processed (includes retry-pending with future `next_retry_at`) |
| `processing` | Jobs currently being processed by the worker |
| `failed` | Jobs that exhausted all retry attempts |
| `done_today` | Jobs completed since midnight UTC today |

## Common Questions

**Q: What happens if Ollama goes down while jobs are in the queue?**
A: Jobs that depend on Ollama (`ai_classify`, `chat_summarize`, `pref_extract`) will fail and be retried with exponential backoff. If Ollama remains down through all 4 attempts, the jobs are permanently marked as `failed`. The corresponding message's `ai_status` is also set to `failed`. Once Ollama recovers, new jobs enqueued from subsequent syncs will process normally. Already-failed jobs are not automatically retried.

**Q: What happens if Memories goes down while jobs are in the queue?**
A: The `memories_store` job will fail and retry. If Memories remains unavailable through all attempts, the job is marked `failed` and `messages.memories_status` is set to `failed`. The `chat_summarize` and `pref_extract` jobs also depend on Memories for storage and will fail similarly. Email sync continues regardless; only the async storage step is affected.

**Q: How do I re-process failed jobs?**
A: There is no built-in retry-all endpoint. To retry specific failed jobs, manually update them in SQLite:
```sql
UPDATE processing_jobs
SET status = 'pending', attempts = 0, next_retry_at = unixepoch()
WHERE status = 'failed';
```
The worker will pick them up on the next poll cycle.

**Q: Can the queue back up during large syncs?**
A: Yes. Initial sync of 100 messages enqueues 200 jobs (one `ai_classify` + one `memories_store` per message). At 4 concurrent workers, each taking 1-5 seconds, processing the full batch takes roughly 1-4 minutes depending on model speed and Memories latency. The queue status endpoint shows real-time counts.

**Q: Does the worker survive server restarts?**
A: The worker itself does not persist; it is spawned at server startup. However, the job queue is persisted in SQLite. Jobs that were `processing` when the server shut down remain in `processing` status. On next startup, these jobs will not be re-claimed (they are not `pending`). To recover them:
```sql
UPDATE processing_jobs SET status = 'pending' WHERE status = 'processing';
```

**Q: How do I monitor queue health?**
A: Call `GET /api/ai/queue-status`. Key indicators: `failed > 0` means some jobs exhausted retries (check connectivity to Ollama/Memories). `processing` remaining high over time may indicate stuck jobs or slow Ollama responses. `pending` growing faster than `done_today` means the worker cannot keep up.

## Related Links

- Source: `src/jobs/queue.rs`, `src/jobs/worker.rs`, `src/jobs/mod.rs`
- API: `src/api/queue_status.rs`
- Migration: `migrations/005_job_queue.sql`
- Sync integration: `src/imap/sync.rs` (enqueue calls)
- Chat integration: `src/api/chat.rs` (chat_summarize trigger)
- Feedback integration: `src/api/ai_feedback.rs` (pref_extract trigger)
