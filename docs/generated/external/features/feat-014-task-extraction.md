---
id: feat-014
type: feature-doc
audience: external
topic: task-extraction
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Task Extraction

## Overview

Task Extraction uses AI to scan an email or thread and pull out actionable items — things someone needs to do, review, send, or decide. Each extracted task includes a description, an optional deadline (if one is mentioned in the email), and a priority level.

Use this to quickly turn a dense email into a structured action list without reading line by line.

---

## How to Use It

1. Identify the thread ID or message ID you want to analyze. You can provide one or both.
2. Send a POST request to `/api/ai/extract-tasks` with at least one of `thread_id` or `message_id`.
3. The response returns an array of tasks. If no actionable items are found, the array is empty.

**Tips:**
- Use `thread_id` to analyze the full conversation context, which gives the AI more signal for deadlines and ownership.
- Use `message_id` to analyze a single message — useful when you want tasks from a specific email in a long thread.

---

## Configuration

Task Extraction requires an AI provider to be configured and enabled in Settings. Supported providers include Ollama (local), Anthropic, and OpenAI. See [AI Configuration](../concepts/ai-configuration.md) for setup instructions.

---

## Examples

### Extract tasks from a thread

```bash
curl -X POST http://localhost:3000/api/ai/extract-tasks \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"thread_id": "thread_abc123"}'
```

**Response:**

```json
{
  "tasks": [
    {
      "description": "Review the Q3 budget spreadsheet and send comments",
      "deadline": "2026-03-15",
      "priority": "high"
    },
    {
      "description": "Schedule a follow-up call with the finance team",
      "deadline": null,
      "priority": "normal"
    },
    {
      "description": "Confirm venue booking for the all-hands",
      "deadline": "2026-03-14",
      "priority": "urgent"
    }
  ]
}
```

---

### Extract tasks from a single message

```bash
curl -X POST http://localhost:3000/api/ai/extract-tasks \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"message_id": "msg_xyz789"}'
```

**Response:**

```json
{
  "tasks": [
    {
      "description": "Send over the revised proposal draft",
      "deadline": "2026-03-18",
      "priority": "high"
    }
  ]
}
```

---

### Email with no tasks

```bash
curl -X POST http://localhost:3000/api/ai/extract-tasks \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"thread_id": "thread_newsletter001"}'
```

**Response:**

```json
{
  "tasks": []
}
```

---

## Task Fields

| Field | Type | Description |
|---|---|---|
| `description` | string | What needs to be done, as extracted from the email |
| `deadline` | string or null | ISO 8601 date (`YYYY-MM-DD`) if a deadline is mentioned, otherwise `null` |
| `priority` | string | One of `urgent`, `high`, `normal`, or `low` |

---

## Limitations

- Requires a configured and reachable AI provider. If AI is unavailable, the endpoint returns a 503 error.
- At least one of `thread_id` or `message_id` must be provided. Sending neither returns a 400 error.
- The AI reads email text only — it does not parse attachments or calendar invites for deadlines.
- Deadline detection depends on explicit date references in the email. Vague language like "soon" or "next week" will not produce a `deadline` value.
- Priority is inferred from language cues (e.g., "urgent", "ASAP", "when you get a chance") and may not always match your own priority judgment.
- Task Extraction does not create tasks in any external task manager. The response is data-only; integration with tools like Todoist or Jira is not built in.

---

## Related

- [Thread Notes (feat-011)](feat-011-thread-notes.md)
- [AI Configuration](../concepts/ai-configuration.md)
- [Multi-Option Reply (feat-013)](feat-013-multi-reply.md)
