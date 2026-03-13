---
id: feat-017
type: feature-doc
audience: external
topic: intent-detection
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# Intent Detection

## Overview

Intent Detection reads an incoming email and classifies what the sender is trying to accomplish — whether that is requesting an action, asking a question, sharing information, scheduling a meeting, or something else entirely. Each message gets an intent label and a confidence score, displayed as a colored badge in your inbox so you can triage at a glance without opening every thread.

You can trigger classification on demand for any message or retrieve a previously stored result. Seven intent classes are supported: `action_request`, `question`, `fyi`, `scheduling`, `sales`, `social`, and `newsletter`.

---

## Getting Started

1. Pick any message in your inbox and note its `message_id` (visible in the messages API response as `"id"`).
2. Call `POST /api/ai/detect-intent` with that ID.
3. The inbox will automatically show the resulting badge the next time it renders. You can also call `GET /api/messages/{id}/intent` at any time to read the stored result without re-running the model.

No additional configuration is required beyond having an AI provider configured in Settings.

---

## API Reference

### Classify intent for a message

```
POST /api/ai/detect-intent
```

**Request body:**

```json
{
  "message_id": "msg_abc123"
}
```

**Example:**

```bash
curl -X POST http://localhost:3000/api/ai/detect-intent \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"message_id": "msg_abc123"}'
```

**Response:**

```json
{
  "message_id": "msg_abc123",
  "intent": "action_request",
  "confidence": 0.91
}
```

`confidence` is a value between `0.0` and `1.0`. Results with confidence below `0.5` should be treated as low-signal.

---

### Retrieve stored intent

```
GET /api/messages/{id}/intent
```

Returns the most recently computed intent for a message without triggering the AI model again.

**Example:**

```bash
curl http://localhost:3000/api/messages/msg_abc123/intent \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "message_id": "msg_abc123",
  "intent": "action_request",
  "confidence": 0.91,
  "computed_at": "2026-03-13T10:22:00Z"
}
```

Returns `404` if no intent has been computed yet for that message.

---

## Intent Classes

| Value | Badge color | Meaning |
|---|---|---|
| `action_request` | error (red) | The sender is asking you to do something |
| `question` | warning (amber) | The sender is asking you a direct question |
| `fyi` | info (blue) | Informational, no response expected |
| `scheduling` | success (green) | The email is about arranging a meeting or time |
| `sales` | text_faint (muted) | Commercial pitch or product outreach |
| `social` | success (green) | Casual or relationship-building message |
| `newsletter` | text_faint (muted) | Bulk broadcast content |

---

## Examples

### Triage a batch of incoming messages

After a sync run you can classify a set of messages quickly:

```bash
for ID in msg_1 msg_2 msg_3; do
  curl -s -X POST http://localhost:3000/api/ai/detect-intent \
    -H "x-session-token: $TOKEN" \
    -H "Content-Type: application/json" \
    -d "{\"message_id\": \"$ID\"}"
done
```

Each call is independent and can be fired concurrently. Results are cached so subsequent reads use `GET /api/messages/{id}/intent`.

### Check whether an old email needed a reply

```bash
curl http://localhost:3000/api/messages/msg_old_thread/intent \
  -H "x-session-token: $TOKEN"
```

If the result is `action_request` or `question` and you never replied, this pairs well with Follow-Up Reminders (feat-022) to surface the gap.

### Build a filtered view of action items

Combine intent data with the messages list. After classifying all messages in a folder, filter your inbox results client-side by `intent == "action_request"` to build a lightweight task queue.

---

## FAQ

**Will classification run automatically on new emails?**
Not by default. Call `POST /api/ai/detect-intent` explicitly, or wire it into your sync workflow. Automatic background classification can be enabled once the job queue worker is configured.

**What happens if the AI provider is unavailable?**
The endpoint returns a `503` with `{"error": "ai_unavailable"}`. Previously computed results stored via `GET /api/messages/{id}/intent` remain accessible.

**Can I override a classification I disagree with?**
There is no manual override endpoint in this version. Re-running `POST /api/ai/detect-intent` on the same message will replace the stored result with a fresh classification.

---

## Related

- [Task Extraction (feat-014)](feat-014-task-extraction.md)
- [Follow-Up Reminders (feat-022)](feat-022-followup-reminders.md)
- [AI Classification (feat-003)](feat-003-ai-classification.md)
