---
id: feat-011
type: feature-doc
audience: external
topic: thread-notes
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Thread Notes

## Overview

Thread Notes lets you attach private, freeform notes to any email thread. Notes are visible only to you and are never sent to anyone. Use them to capture follow-up reminders, context from a phone call, or anything else you want to remember about a conversation.

Each note belongs to a single thread. You can create multiple notes per thread, update them, and delete them at any time.

---

## How to Use It

### Create a note

1. Identify the thread ID from the thread list or thread detail response.
2. Send a POST request to `/api/threads/{thread_id}/notes` with your note content.
3. The response returns the newly created note, including its ID and timestamps.

### Read notes on a thread

Send a GET request to `/api/threads/{thread_id}/notes`. Notes are returned in reverse chronological order (newest first).

### Update a note

Send a PUT request to `/api/threads/{thread_id}/notes/{note_id}` with the new content. The full note content is replaced — partial updates are not supported.

### Delete a note

Send a DELETE request to `/api/threads/{thread_id}/notes/{note_id}`. The note is permanently removed.

---

## Configuration

No additional configuration is required. Thread Notes are available to all authenticated users.

---

## Examples

### Create a note

```bash
curl -X POST http://localhost:3000/api/threads/thread_abc123/notes \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"content": "Called Alice — she needs the contract by Friday EOD."}'
```

**Response:**

```json
{
  "id": "note_789xyz",
  "thread_id": "thread_abc123",
  "content": "Called Alice — she needs the contract by Friday EOD.",
  "created_at": "2026-03-13T10:22:00Z",
  "updated_at": "2026-03-13T10:22:00Z"
}
```

---

### List notes on a thread

```bash
curl http://localhost:3000/api/threads/thread_abc123/notes \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response:**

```json
[
  {
    "id": "note_789xyz",
    "thread_id": "thread_abc123",
    "content": "Called Alice — she needs the contract by Friday EOD.",
    "created_at": "2026-03-13T10:22:00Z",
    "updated_at": "2026-03-13T10:22:00Z"
  }
]
```

---

### Update a note

```bash
curl -X PUT http://localhost:3000/api/threads/thread_abc123/notes/note_789xyz \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"content": "Contract sent — waiting on countersignature."}'
```

**Response:**

```json
{
  "id": "note_789xyz",
  "thread_id": "thread_abc123",
  "content": "Contract sent — waiting on countersignature.",
  "created_at": "2026-03-13T10:22:00Z",
  "updated_at": "2026-03-13T14:05:00Z"
}
```

---

### Delete a note

```bash
curl -X DELETE http://localhost:3000/api/threads/thread_abc123/notes/note_789xyz \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

Returns `204 No Content` on success.

---

## Limitations

- Note content is limited to 10,000 characters.
- Notes are stored per thread, not per individual message.
- Deleting a thread does not automatically surface a warning about attached notes — they will be removed along with the thread.
- Notes are private to your Iris instance. They are not synced to your email provider or shared across accounts.

---

## Related

- [Thread View](../concepts/thread-view.md)
- [Task Extraction (feat-014)](feat-014-task-extraction.md)
