---
id: feat-022
type: feature-doc
audience: external
topic: followup-reminders
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# Follow-Up Reminders

## Overview

Follow-Up Reminders automatically detects emails you sent that have not received a reply, then surfaces them with an urgency level so nothing falls through the cracks. Iris reads your sent mail and identifies threads where a response was expected but has not arrived — whether because the email asked a direct question, requested an action, or contained a time-sensitive matter.

You can snooze reminders to revisit them later or dismiss them when no follow-up is needed. Urgency levels range from `low` to `urgent` so you always know which unanswered emails to chase first.

---

## Getting Started

1. Call `POST /api/ai/scan-followups` to analyze your sent mail and populate the follow-up list.
2. Call `GET /api/followups` to see which emails are awaiting a response.
3. Use `PUT /api/followups/{id}/snooze` or `PUT /api/followups/{id}/dismiss` to manage each reminder.

Run the scan periodically (daily is a good cadence) to keep the list current.

---

## API Reference

### Scan for follow-ups

```
POST /api/ai/scan-followups
```

Analyzes recently sent emails for unanswered messages. The scan looks at your outbox and cross-references replies in the same threads. Results are stored and available via `GET /api/followups`.

**Example:**

```bash
curl -X POST http://localhost:3000/api/ai/scan-followups \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "status": "ok",
  "followups_found": 5,
  "scan_completed_at": "2026-03-13T08:00:00Z"
}
```

---

### List follow-up reminders

```
GET /api/followups
```

Returns all active (non-dismissed) follow-ups, sorted by urgency descending.

**Example:**

```bash
curl http://localhost:3000/api/followups \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "followups": [
    {
      "id": "fu_001",
      "thread_id": "thr_contract",
      "message_id": "msg_sent_003",
      "subject": "Re: Q2 Contract Review",
      "recipient": "alice@example.com",
      "sent_at": "2026-03-10T14:20:00Z",
      "days_waiting": 3,
      "urgency": "high",
      "reason": "Sent email contains a deadline and no reply has been received",
      "snoozed_until": null,
      "dismissed": false
    },
    {
      "id": "fu_002",
      "thread_id": "thr_intro",
      "message_id": "msg_sent_007",
      "subject": "Introduction — follow up",
      "recipient": "bob@partner.org",
      "sent_at": "2026-03-06T09:00:00Z",
      "days_waiting": 7,
      "urgency": "normal",
      "reason": "No reply received after 7 days",
      "snoozed_until": null,
      "dismissed": false
    }
  ],
  "total": 2
}
```

---

### Snooze a reminder

```
PUT /api/followups/{id}/snooze
```

Hides a reminder until the specified date. On that date it reappears in `GET /api/followups` results.

**Request body:**

```json
{
  "until": "2026-03-18"
}
```

Date must be in `YYYY-MM-DD` format and must be in the future.

**Example:**

```bash
curl -X PUT http://localhost:3000/api/followups/fu_002/snooze \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"until": "2026-03-18"}'
```

**Response:**

```json
{
  "id": "fu_002",
  "snoozed_until": "2026-03-18",
  "updated_at": "2026-03-13T11:30:00Z"
}
```

---

### Dismiss a reminder

```
PUT /api/followups/{id}/dismiss
```

Permanently removes a reminder from your list. Use this when you have followed up by phone, Slack, or another channel, and no email reply is expected.

**Example:**

```bash
curl -X PUT http://localhost:3000/api/followups/fu_001/dismiss \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "id": "fu_001",
  "dismissed": true,
  "dismissed_at": "2026-03-13T11:35:00Z"
}
```

---

## Urgency Levels

| Level | Badge color | Meaning |
|---|---|---|
| `urgent` | error (red) | More than 5 days waiting, email contained a deadline or explicit time pressure |
| `high` | warning (amber) | 3–5 days waiting, or email contained an action request or question |
| `normal` | success (green) | 1–3 days waiting with expected reply |
| `low` | text_faint (muted) | Less than 1 day, or a social/informational message with no strong expectation of reply |

Urgency is determined by a combination of how long you have been waiting and the intent of the original message (from Intent Detection, if available).

---

## Examples

### Morning triage routine

Scan for new follow-ups, then review urgent ones first:

```bash
curl -X POST http://localhost:3000/api/ai/scan-followups \
  -H "x-session-token: $TOKEN"

curl http://localhost:3000/api/followups \
  -H "x-session-token: $TOKEN"
```

Filter client-side to `urgency == "urgent"` or `urgency == "high"` to focus your morning.

### Snooze over a weekend

If a reminder comes up on a Friday for a non-urgent email, snooze it to Monday:

```bash
curl -X PUT http://localhost:3000/api/followups/fu_003/snooze \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"until": "2026-03-16"}'
```

### Dismiss after a call

You followed up with a client by phone and no email reply is coming:

```bash
curl -X PUT http://localhost:3000/api/followups/fu_001/dismiss \
  -H "x-session-token: $TOKEN"
```

---

## FAQ

**Will a follow-up be automatically resolved if a reply arrives?**
Yes. When Iris syncs new email, it checks whether any threads with active follow-ups have received a reply. If so, the follow-up is automatically marked resolved and removed from `GET /api/followups` results.

**Does snoozing affect urgency?**
No. Urgency is recalculated based on how long the original sent email has been waiting for a reply. A snoozed reminder may come back at a higher urgency level if more days have passed.

**Can I see dismissed reminders?**
Dismissed reminders are not returned by `GET /api/followups` by default. This endpoint currently has no `include_dismissed` parameter — dismissed records are treated as permanently resolved.

---

## Related

- [Intent Detection (feat-017)](feat-017-intent-detection.md)
- [Deadline Extraction (feat-018)](feat-018-deadline-extraction.md)
- [Task Extraction (feat-014)](feat-014-task-extraction.md)
