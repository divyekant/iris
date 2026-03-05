---
id: feat-009
title: Reliable Email Processing
audience: external
generated: 2026-03-04
status: current
---

# Reliable Email Processing

Iris now processes your emails through a reliable background job queue with automatic retry. AI classification, memory storage, and other background tasks no longer silently fail when something goes temporarily wrong.

## What This Means for You

Previously, if Ollama was briefly unavailable or the Memories service had a hiccup during email sync, those processing steps were skipped without any indication. You would end up with unclassified messages or gaps in your searchable email archive.

Now, every background task -- AI classification, memory storage, chat summarization, and preference extraction -- is queued as a job. If a job fails (for example, because Ollama is restarting), it is automatically retried with increasing delays. Once the service comes back, the queued work catches up on its own.

In practice, this means:

- **AI classifications are not lost.** If Ollama is temporarily unavailable when a new email arrives, classification is retried until it succeeds.
- **Memories storage is resilient.** Transient network issues do not leave gaps in your semantic search index.
- **No silent failures.** Failed jobs are tracked and visible through the queue status endpoint.

## Checking Queue Status

You can check the current state of the background job queue at any time:

```
GET /api/ai/queue-status
```

**Example request:**

```bash
curl -s -H "X-Session-Token: your_session_token" \
  "http://localhost:3000/api/ai/queue-status"
```

**Response:**

```json
{
  "pending": 3,
  "processing": 1,
  "failed": 0,
  "done_today": 142
}
```

| Field | Description |
|---|---|
| `pending` | Jobs waiting to be processed |
| `processing` | Jobs currently running |
| `failed` | Jobs that have exceeded retry attempts |
| `done_today` | Jobs successfully completed since midnight |

If you see a growing `pending` count, it usually means one of the backend services (Ollama or Memories) is unavailable. Check their connectivity in **Settings**.

## Configuration

You can tune the job queue behavior with these environment variables:

| Variable | Type | Default | Description |
|---|---|---|---|
| `JOB_POLL_INTERVAL_MS` | number | `2000` | How often (in milliseconds) the job worker checks for new jobs to process |
| `JOB_MAX_CONCURRENCY` | number | `4` | Maximum number of jobs processed at the same time |
| `JOB_CLEANUP_DAYS` | number | `7` | Number of days to keep completed job records before automatic cleanup |

The defaults work well for most setups. You might lower `JOB_POLL_INTERVAL_MS` if you want faster processing at the cost of slightly higher CPU usage, or increase `JOB_MAX_CONCURRENCY` if you have a powerful machine and want to process large backlogs faster.

## What Gets Queued

The following tasks run through the job queue:

- **AI classification** -- Categorizing incoming emails by intent, priority, and category
- **Memories storage** -- Storing email content in the Memories vector store for semantic search
- **Chat summarization** -- Generating session summaries for the AI chat assistant
- **Preference extraction** -- Learning your classification preferences from feedback corrections
