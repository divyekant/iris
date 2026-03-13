---
id: feat-020
type: feature-doc
audience: external
topic: context-autocomplete
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# Context Autocomplete

## Overview

Context Autocomplete generates short text continuations while you compose a message. It reads the thread you are replying to along with whatever you have typed so far, then offers up to three suggestions that fit naturally into what you are writing. Unlike generic autocomplete, the suggestions are grounded in the actual conversation context — the names, dates, and specifics that were already discussed in the thread.

You can use it when drafting a new message, replying, replying to all, or forwarding. The suggestions arrive with confidence scores so you can pick the most relevant one or ignore them all.

---

## Getting Started

1. Open the compose modal for any thread.
2. Start typing your message. When you want suggestions, note the current `thread_id`, `partial_text`, and `cursor_position` (character offset from the start of the text).
3. Call `POST /api/ai/autocomplete`. You will receive up to three continuations ranked by confidence.
4. Insert the one you want at the cursor position, or discard them all and keep typing.

---

## API Reference

### Get autocomplete suggestions

```
POST /api/ai/autocomplete
```

**Request body:**

| Field | Type | Required | Description |
|---|---|---|---|
| `thread_id` | string | yes | The thread being composed in |
| `partial_text` | string | yes | The full compose body as typed so far |
| `cursor_position` | integer | yes | Character offset where the cursor sits |
| `compose_mode` | string | yes | One of: `new`, `reply`, `reply_all`, `forward` |

**Example:**

```bash
curl -X POST http://localhost:3000/api/ai/autocomplete \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "thread_id": "thr_xyz",
    "partial_text": "Thanks for sending over the contract. I have reviewed section 3 and I think we should",
    "cursor_position": 87,
    "compose_mode": "reply"
  }'
```

**Response:**

```json
{
  "suggestions": [
    {
      "text": " schedule a call this week to align on the indemnity clause before we proceed.",
      "confidence": 0.88
    },
    {
      "text": " discuss the payment terms before signing.",
      "confidence": 0.74
    },
    {
      "text": " loop in legal to review the liability section.",
      "confidence": 0.67
    }
  ],
  "thread_id": "thr_xyz",
  "cursor_position": 87
}
```

Each `text` value is the continuation from the cursor position — not a replacement for the whole body. Append it to `partial_text` at `cursor_position` to compose the full result.

---

## Compose Modes

| Mode | Context used |
|---|---|
| `new` | No prior thread context; suggestions are general continuations of `partial_text` |
| `reply` | The thread history is included; suggestions reflect what has already been said |
| `reply_all` | Same as `reply`; useful for group threads |
| `forward` | Thread history is included; suggestions may reference what you are forwarding |

---

## Examples

### Reply to a meeting request

```bash
curl -X POST http://localhost:3000/api/ai/autocomplete \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "thread_id": "thr_meeting_001",
    "partial_text": "Hi Sarah, thanks for reaching out. I am available",
    "cursor_position": 49,
    "compose_mode": "reply"
  }'
```

The model will read the original email from Sarah to understand what meeting is being requested, then generate time options or confirmation text that fits her proposed agenda.

### Compose a new message from scratch

```bash
curl -X POST http://localhost:3000/api/ai/autocomplete \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "thread_id": "thr_new",
    "partial_text": "Hi team, I wanted to give everyone a quick update on the",
    "cursor_position": 57,
    "compose_mode": "new"
  }'
```

With `compose_mode: new`, suggestions rely only on `partial_text` — there is no prior thread context to draw from.

### Forward a document with commentary

```bash
curl -X POST http://localhost:3000/api/ai/autocomplete \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "thread_id": "thr_doc_review",
    "partial_text": "Forwarding the latest draft. Please pay special attention to",
    "cursor_position": 60,
    "compose_mode": "forward"
  }'
```

---

## FAQ

**How many suggestions will I always get?**
The API returns up to three suggestions. If the model is not confident enough to produce three distinct continuations, the array may contain fewer. An empty `suggestions` array means no suitable continuations were found for this input.

**Does cursor position matter if my cursor is at the end of the text?**
Set `cursor_position` to the length of `partial_text`. The model treats the cursor as the insertion point, so everything before it is context and nothing is expected after it.

**Can I use this in a standalone tool outside the Iris compose modal?**
Yes. The API is stateless — it does not require you to be inside the compose modal. As long as you have a valid `thread_id` and `partial_text`, you can call it from any client.

---

## Related

- [AI Writing Assist (feat-004)](feat-004-ai-writing-assist.md)
- [Multi-Option Reply (feat-013)](feat-013-multi-reply.md)
- [Grammar Assist (feat-016)](feat-016-response-times.md)
