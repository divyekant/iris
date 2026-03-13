---
id: feat-018
type: feature-doc
audience: external
topic: deadline-extraction
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# Deadline Extraction

## Overview

Deadline Extraction reads the text of your emails and pulls out any dates or time-bound commitments — whether the sender wrote an explicit date like "March 20" or a relative phrase like "by end of week" or "needs to be done by Friday." Each deadline is stored as a structured record you can query, mark complete, or delete.

This saves you from manually scanning message text to track what is due when, and gives you a queryable list of upcoming deadlines across all threads.

---

## Getting Started

1. Call `POST /api/ai/extract-deadlines` with the `message_id` you want to analyze.
2. Iris parses the message and stores any deadlines it finds.
3. Use `GET /api/deadlines` to list all deadlines, with optional filters to narrow by time window or exclude completed items.

---

## API Reference

### Extract deadlines from a message

```
POST /api/ai/extract-deadlines
```

**Request body:**

```json
{
  "message_id": "msg_abc123"
}
```

**Example:**

```bash
curl -X POST http://localhost:3000/api/ai/extract-deadlines \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message_id": "msg_abc123"}'
```

**Response:**

```json
{
  "message_id": "msg_abc123",
  "deadlines_found": 2,
  "deadlines": [
    {
      "id": "dl_001",
      "description": "Submit the budget report",
      "due_date": "2026-03-20",
      "source_text": "please submit the budget report by March 20",
      "is_completed": false
    },
    {
      "id": "dl_002",
      "description": "Schedule a follow-up call",
      "due_date": "2026-03-15",
      "source_text": "let's schedule a follow-up call by Friday",
      "is_completed": false
    }
  ]
}
```

`due_date` is always resolved to a calendar date in `YYYY-MM-DD` format, even for relative phrases. If the model cannot resolve a relative date (for example, "by next quarter" without a reference date), the deadline is still stored with `due_date: null`.

---

### List all deadlines

```
GET /api/deadlines
```

**Query parameters:**

| Parameter | Type | Default | Description |
|---|---|---|---|
| `days` | integer | — | Return only deadlines due within the next N days |
| `include_completed` | boolean | `false` | Set to `true` to include completed deadlines |

**Example — deadlines due in the next 7 days:**

```bash
curl "http://localhost:3000/api/deadlines?days=7&include_completed=false" \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "deadlines": [
    {
      "id": "dl_002",
      "message_id": "msg_abc123",
      "thread_id": "thr_xyz",
      "description": "Schedule a follow-up call",
      "due_date": "2026-03-15",
      "source_text": "let's schedule a follow-up call by Friday",
      "is_completed": false,
      "extracted_at": "2026-03-13T09:00:00Z"
    }
  ],
  "total": 1
}
```

---

### List deadlines for a specific thread

```
GET /api/threads/{id}/deadlines
```

**Example:**

```bash
curl http://localhost:3000/api/threads/thr_xyz/deadlines \
  -H "x-session-token: $TOKEN"
```

Returns all deadlines extracted from any message in that thread.

---

### Mark a deadline complete

```
PUT /api/deadlines/{id}/complete
```

**Example:**

```bash
curl -X PUT http://localhost:3000/api/deadlines/dl_002/complete \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "id": "dl_002",
  "is_completed": true,
  "completed_at": "2026-03-13T14:30:00Z"
}
```

---

### Delete a deadline

```
DELETE /api/deadlines/{id}
```

Use this to remove a deadline that was incorrectly extracted or is no longer relevant.

**Example:**

```bash
curl -X DELETE http://localhost:3000/api/deadlines/dl_001 \
  -H "x-session-token: $TOKEN"
```

Returns `204 No Content` on success.

---

## Examples

### Daily standup: what's due this week?

```bash
curl "http://localhost:3000/api/deadlines?days=5" \
  -H "x-session-token: $TOKEN"
```

Run this each morning to see everything due in the next five working days.

### Process an entire thread for deadlines

```bash
# Get message IDs from a thread first
curl http://localhost:3000/api/threads/thr_xyz \
  -H "x-session-token: $TOKEN"

# Then extract deadlines from the key message
curl -X POST http://localhost:3000/api/ai/extract-deadlines \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message_id": "msg_key_message"}'

# And review all deadlines for that thread
curl http://localhost:3000/api/threads/thr_xyz/deadlines \
  -H "x-session-token: $TOKEN"
```

### Clean up after completing work

After finishing a task mentioned in a thread, mark it done and confirm nothing else is pending:

```bash
curl -X PUT http://localhost:3000/api/deadlines/dl_001/complete \
  -H "x-session-token: $TOKEN"

curl "http://localhost:3000/api/threads/thr_xyz/deadlines" \
  -H "x-session-token: $TOKEN"
```

---

## FAQ

**Can Iris extract multiple deadlines from one email?**
Yes. A single email can produce several deadline records if it mentions multiple due dates or commitments. The response from `POST /api/ai/extract-deadlines` includes all of them in the `deadlines` array.

**What happens with vague phrases like "as soon as possible"?**
The model records these as deadlines with `due_date: null` and a description that captures the urgency. They appear in `GET /api/deadlines` results without a specific date — you can review and delete them if not useful.

**Does extraction run automatically on incoming email?**
Not by default. You call `POST /api/ai/extract-deadlines` explicitly for each message you want analyzed. Pairing this with the job queue lets you automate extraction on sync.

---

## Related

- [Task Extraction (feat-014)](feat-014-task-extraction.md)
- [Follow-Up Reminders (feat-022)](feat-022-followup-reminders.md)
- [Thread View](../concepts/thread-view.md)
