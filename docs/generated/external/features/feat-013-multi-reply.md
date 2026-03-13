---
id: feat-013
type: feature-doc
audience: external
topic: multi-option-reply
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Multi-Option Reply

## Overview

Multi-Option Reply uses AI to generate three ready-to-send reply drafts for an email thread, each in a different tone: formal, casual, and brief. You choose the one that fits, or use it as a starting point and edit from there.

The optional `context` field lets you steer the AI — for example, to decline a meeting request, push back on a deadline, or ask a follow-up question — so all three options reflect your intent while varying in tone.

---

## How to Use It

1. Find the thread ID for the conversation you want to reply to.
2. Send a POST request to `/api/ai/multi-reply` with the thread ID.
3. Optionally include a `context` string to guide the AI (e.g., "accept the proposal but ask for more time").
4. Review the three returned options. Each includes a suggested subject line and full reply body.
5. Copy the body you prefer into your compose window, adjust as needed, and send.

---

## Configuration

Multi-Option Reply requires an AI provider to be configured and enabled in Settings. Supported providers include Ollama (local), Anthropic, and OpenAI. See [AI Configuration](../concepts/ai-configuration.md) for setup instructions.

---

## Examples

### Generate replies with no extra context

```bash
curl -X POST http://localhost:3000/api/ai/multi-reply \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"thread_id": "thread_abc123"}'
```

**Response:**

```json
{
  "options": [
    {
      "tone": "formal",
      "subject": "Re: Q3 Budget Review",
      "body": "Dear Sarah,\n\nThank you for your message. I have reviewed the Q3 budget figures and will have my comments to you by end of business Friday.\n\nBest regards,\nAlex"
    },
    {
      "tone": "casual",
      "subject": "Re: Q3 Budget Review",
      "body": "Hey Sarah,\n\nGot it — I'll take a look and send over my notes by Friday. Thanks!\n\nAlex"
    },
    {
      "tone": "brief",
      "subject": "Re: Q3 Budget Review",
      "body": "Will review and reply by Friday EOD."
    }
  ]
}
```

---

### Generate replies with guided context

```bash
curl -X POST http://localhost:3000/api/ai/multi-reply \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{
    "thread_id": "thread_xyz789",
    "context": "decline politely, mention a scheduling conflict next week"
  }'
```

**Response:**

```json
{
  "options": [
    {
      "tone": "formal",
      "subject": "Re: Meeting Request — March 18",
      "body": "Dear Marcus,\n\nThank you for reaching out. Unfortunately, I have a prior commitment next week that prevents me from attending on March 18. I would be happy to find an alternative time — please let me know your availability for the following week.\n\nBest regards,\nJamila"
    },
    {
      "tone": "casual",
      "subject": "Re: Meeting Request — March 18",
      "body": "Hi Marcus,\n\nAh, bad timing — I'm already booked solid next week. Can we move it to the week after? Happy to find a slot that works for you.\n\nJamila"
    },
    {
      "tone": "brief",
      "subject": "Re: Meeting Request — March 18",
      "body": "Conflicted next week — can we reschedule to the week of March 25?"
    }
  ]
}
```

---

### Reply to a thread with no prior messages (edge case)

If the thread contains no messages, the AI returns a graceful fallback with generic starter options based on the subject line alone.

```bash
curl -X POST http://localhost:3000/api/ai/multi-reply \
  -H "Content-Type: application/json" \
  -H "x-session-token: YOUR_SESSION_TOKEN" \
  -d '{"thread_id": "thread_new001", "context": "acknowledge receipt"}'
```

---

## Limitations

- Requires a configured and reachable AI provider. If AI is unavailable, the endpoint returns a 503 error.
- The `context` field accepts free text up to a reasonable length; very long context strings may be truncated before being passed to the model.
- The AI reads the thread content but does not access attachments.
- All three tone options are generated in a single request. You cannot request only one tone.
- Generated replies may reflect the model's phrasing conventions — always review before sending.

---

## Related

- [AI Assist in Compose](../concepts/ai-assist-compose.md)
- [AI Configuration](../concepts/ai-configuration.md)
- [Task Extraction (feat-014)](feat-014-task-extraction.md)
