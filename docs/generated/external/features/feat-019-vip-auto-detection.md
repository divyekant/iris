---
id: feat-019
type: feature-doc
audience: external
topic: vip-auto-detection
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# VIP Auto-Detection

## Overview

VIP Auto-Detection analyzes your email history to surface the contacts who matter most to you — not based on a static list you maintain, but on actual communication patterns. It scores each contact across four signals: how often you exchange messages, how consistently each side replies, how recently you were in contact, and how deep your threads run. The result is a ranked list of VIP contacts you can use to prioritize notifications, triage, and follow-up.

You can also manually flag any contact as VIP (or remove the flag) regardless of their computed score.

---

## Getting Started

1. Call `POST /api/contacts/vip/compute` to run the scoring algorithm over your full email history. This is a one-time or periodic operation — you can re-run it as your inbox grows.
2. Call `GET /api/contacts/vip` to see the ranked list.
3. Use `PUT /api/contacts/{email}/vip` to manually promote or demote any contact.

---

## API Reference

### Compute VIP scores

```
POST /api/contacts/vip/compute
```

Triggers a full recomputation of VIP scores across all contacts with email history. This is an async operation — the response confirms the job was queued rather than waiting for completion.

**Example:**

```bash
curl -X POST http://localhost:3000/api/contacts/vip/compute \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "status": "queued",
  "message": "VIP score computation started. Check GET /api/contacts/vip for results."
}
```

---

### List VIP contacts

```
GET /api/contacts/vip
```

Returns all contacts currently marked as VIP, sorted by score descending. Includes both auto-detected contacts (where `auto_detected` is `true`) and any you have manually promoted.

**Example:**

```bash
curl http://localhost:3000/api/contacts/vip \
  -H "x-session-token: $TOKEN"
```

**Response:**

```json
{
  "vip_contacts": [
    {
      "email": "alice@example.com",
      "display_name": "Alice Chen",
      "vip_score": 0.94,
      "auto_detected": true,
      "manually_set": false,
      "score_breakdown": {
        "frequency": 0.95,
        "reply_rate": 0.92,
        "recency": 0.98,
        "thread_depth": 0.88
      },
      "last_contact": "2026-03-12T16:40:00Z"
    },
    {
      "email": "bob@partner.org",
      "display_name": "Bob Tanaka",
      "vip_score": 0.61,
      "auto_detected": false,
      "manually_set": true,
      "score_breakdown": null,
      "last_contact": "2026-02-28T09:15:00Z"
    }
  ],
  "total": 2
}
```

`score_breakdown` is `null` for manually-set VIPs who were added before any score was computed.

---

### Manually set VIP status

```
PUT /api/contacts/{email}/vip
```

The `{email}` segment must be URL-encoded. For example, `alice@example.com` becomes `alice%40example.com`.

**Request body:**

```json
{
  "is_vip": true
}
```

Set `"is_vip": false` to remove VIP status.

**Example — promote a contact:**

```bash
curl -X PUT http://localhost:3000/api/contacts/bob%40partner.org/vip \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"is_vip": true}'
```

**Response:**

```json
{
  "email": "bob@partner.org",
  "is_vip": true,
  "manually_set": true,
  "updated_at": "2026-03-13T11:00:00Z"
}
```

**Example — remove VIP status:**

```bash
curl -X PUT http://localhost:3000/api/contacts/bob%40partner.org/vip \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"is_vip": false}'
```

---

## Scoring Signals

VIP scores are computed on a 0–1 scale using four equally weighted signals:

| Signal | What it measures |
|---|---|
| `frequency` | How often you exchange emails with this contact |
| `reply_rate` | What fraction of messages in both directions receive a reply |
| `recency` | How recently you were in contact (decays over time) |
| `thread_depth` | Average number of back-and-forth messages in your shared threads |

The final `vip_score` is a weighted combination of these four values. Contacts with scores above approximately `0.75` are automatically promoted to VIP status.

---

## Examples

### Bootstrap VIP detection after initial sync

Run this once after your inbox is populated:

```bash
curl -X POST http://localhost:3000/api/contacts/vip/compute \
  -H "x-session-token: $TOKEN"

# Wait a moment, then check results
curl http://localhost:3000/api/contacts/vip \
  -H "x-session-token: $TOKEN"
```

### Demote a high-volume vendor you do not want flagged

A mailing list sender may score high on frequency despite not being personally important:

```bash
curl -X PUT http://localhost:3000/api/contacts/vendor-updates%40example.com/vip \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"is_vip": false}'
```

This sets `manually_set: true` and pins the status to `false` — a future `compute` run will not override a manual setting.

### Promote a new hire whose score has not built up yet

```bash
curl -X PUT http://localhost:3000/api/contacts/newcolleague%40company.com/vip \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"is_vip": true}'
```

---

## FAQ

**How often should I re-run `POST /api/contacts/vip/compute`?**
Weekly or after large sync batches is a good cadence. The computation only looks at stored email history, so it stays fast even on large inboxes.

**Will a manual VIP setting be overwritten when I re-run compute?**
No. Contacts where `manually_set` is `true` are not affected by score recomputation. Your manual settings always take priority.

**My most important contact has a low score. Why?**
Scores reflect two-way communication patterns. If a key executive rarely replies to you directly but you always reply to them, their `reply_rate` signal will be low. Use the manual VIP toggle to promote contacts that your communication style does not naturally surface.

---

## Related

- [Response Times (feat-016)](feat-016-response-times.md)
- [Key Topics (feat-015)](feat-015-key-topics.md)
- [AI Classification (feat-003)](feat-003-ai-classification.md)
