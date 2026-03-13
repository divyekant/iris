---
id: feat-016
type: feature-doc
audience: external
topic: response-times
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Response Times

## Overview

Response Times gives you reply latency statistics for any contact — both their responsiveness to your emails and your responsiveness to theirs. At a glance you can see average reply times in each direction, the fastest and slowest exchanges on record, and the total number of back-and-forth replies analyzed.

Use this to understand communication patterns: is a vendor consistently slow? Are you reliably fast with a key stakeholder?

---

## How to Use It

1. Identify the contact's email address (URL-encode it if it contains special characters).
2. Send a GET request to `/api/contacts/{email}/response-times`.
3. The response includes reply averages for both sides of the conversation, as well as fastest, slowest, and total exchange counts.

Fields that require reply data will be `null` if no reply pairs exist yet (e.g., a contact you have never exchanged back-and-forth emails with).

---

## Configuration

No AI configuration is required. Response Times are calculated from your email history and are always available for any contact with reply data.

---

## Examples

### Get response time stats for a contact

```bash
curl "http://localhost:3000/api/contacts/alice%40example.com/response-times" \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response:**

```json
{
  "email": "alice@example.com",
  "their_avg_reply_hours": 2.5,
  "your_avg_reply_hours": 1.8,
  "their_reply_count": 15,
  "your_reply_count": 12,
  "fastest_reply_hours": 0.1,
  "slowest_reply_hours": 48.0,
  "total_exchanges": 30
}
```

In this example, Alice replies in about 2.5 hours on average, and you reply to her in about 1.8 hours. The fastest reply in either direction was 6 minutes (0.1 hours), and the slowest was 48 hours.

---

### Contact with email address containing special characters

```bash
curl "http://localhost:3000/api/contacts/bob%2Bwork%40example.com/response-times" \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response:**

```json
{
  "email": "bob+work@example.com",
  "their_avg_reply_hours": 0.8,
  "your_avg_reply_hours": 4.2,
  "their_reply_count": 9,
  "your_reply_count": 8,
  "fastest_reply_hours": 0.05,
  "slowest_reply_hours": 22.3,
  "total_exchanges": 17
}
```

---

### Contact with no reply history

```bash
curl "http://localhost:3000/api/contacts/newsletter%40example.com/response-times" \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response:**

```json
{
  "email": "newsletter@example.com",
  "their_avg_reply_hours": null,
  "your_avg_reply_hours": null,
  "their_reply_count": 0,
  "your_reply_count": 0,
  "fastest_reply_hours": null,
  "slowest_reply_hours": null,
  "total_exchanges": 0
}
```

All time fields are `null` when there are no reply pairs to measure.

---

## Response Fields

| Field | Type | Description |
|---|---|---|
| `email` | string | The contact's email address |
| `their_avg_reply_hours` | number or null | Average time (in hours) for this contact to reply to your emails |
| `your_avg_reply_hours` | number or null | Average time (in hours) for you to reply to this contact |
| `their_reply_count` | number | Number of times this contact has replied to you |
| `your_reply_count` | number | Number of times you have replied to this contact |
| `fastest_reply_hours` | number or null | The shortest reply time observed in either direction |
| `slowest_reply_hours` | number or null | The longest reply time observed in either direction |
| `total_exchanges` | number | Total number of reply pairs analyzed |

All time values are in decimal hours. For example, `0.5` means 30 minutes.

---

## Limitations

- The contact email must be URL-encoded in the path. For example, `bob+work@example.com` becomes `bob%2Bwork%40example.com`.
- Statistics are calculated from emails stored in your Iris inbox. Emails that were not synced will not be included.
- Reply time is measured between consecutive messages in a thread where one message is from you and the next is from the contact (or vice versa). Automated replies and delivery receipts may affect accuracy.
- `fastest_reply_hours` and `slowest_reply_hours` are calculated across all exchanges with that contact in both directions — they do not distinguish whose reply was fastest or slowest.
- For contacts you only receive email from (and never reply to), `your_avg_reply_hours` and `your_reply_count` will be `null` or `0` respectively.

---

## Related

- [Key Topics (feat-015)](feat-015-key-topics.md)
- [Thread View](../concepts/thread-view.md)
- [Keyword Search](../concepts/search.md)
