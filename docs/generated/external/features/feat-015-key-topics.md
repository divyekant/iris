---
id: feat-015
type: feature-doc
audience: external
topic: key-topics
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# Key Topics

## Overview

Key Topics gives you an AI-analyzed summary of the recurring conversation themes you share with a specific contact. For any contact in your inbox, you can see which topics come up most often — such as "Project updates", "Budget discussions", or "Interview scheduling" — along with how many emails relate to each topic.

You can also retrieve a ranked list of your most-frequently-contacted people, making it easy to spot your most active relationships at a glance.

---

## How to Use It

### Get topics for a contact

1. Identify the contact's email address (URL-encode it if it contains special characters).
2. Send a GET request to `/api/contacts/{email}/topics`.
3. The first request triggers an AI analysis of that contact's emails. This may take a few seconds.
4. Results are cached for one hour. Subsequent requests within that window return immediately with `"cached": true`.

### Get your top contacts

Send a GET request to `/api/contacts/top`. This returns your most-contacted people sorted by email frequency, with name and last-contact timestamp where available.

---

## Configuration

Topic analysis requires an AI provider to be configured and enabled in Settings. The top contacts list (`/api/contacts/top`) does not require AI and is always available. See [AI Configuration](../concepts/ai-configuration.md) for provider setup.

---

## Examples

### Get topics for a contact

```bash
curl "http://localhost:3000/api/contacts/alice%40example.com/topics" \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response (first call — analysis in progress, then returns result):**

```json
{
  "email": "alice@example.com",
  "topics": [
    {"name": "Project updates", "count": 12},
    {"name": "Budget planning", "count": 7},
    {"name": "Team hiring", "count": 4}
  ],
  "total_emails": 47,
  "cached": false
}
```

---

### Get topics for a contact (cached)

```bash
curl "http://localhost:3000/api/contacts/alice%40example.com/topics" \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response (within one hour of the first call):**

```json
{
  "email": "alice@example.com",
  "topics": [
    {"name": "Project updates", "count": 12},
    {"name": "Budget planning", "count": 7},
    {"name": "Team hiring", "count": 4}
  ],
  "total_emails": 47,
  "cached": true
}
```

---

### Get your top contacts

```bash
curl http://localhost:3000/api/contacts/top \
  -H "x-session-token: YOUR_SESSION_TOKEN"
```

**Response:**

```json
[
  {
    "email": "alice@example.com",
    "name": "Alice Chen",
    "email_count": 47,
    "last_contact": 1710432000
  },
  {
    "email": "bob@example.com",
    "name": "Bob Sharma",
    "email_count": 31,
    "last_contact": 1710345600
  },
  {
    "email": "carol@example.com",
    "name": null,
    "email_count": 18,
    "last_contact": 1710259200
  }
]
```

`last_contact` is a Unix timestamp (seconds). `name` is `null` if no display name has been observed for that address.

---

## Response Fields

### `/api/contacts/{email}/topics`

| Field | Type | Description |
|---|---|---|
| `email` | string | The contact's email address |
| `topics` | array | List of topics, each with `name` (string) and `count` (number of emails) |
| `total_emails` | number | Total emails exchanged with this contact |
| `cached` | boolean | Whether this result was served from cache (`true`) or freshly analyzed (`false`) |

### `/api/contacts/top`

| Field | Type | Description |
|---|---|---|
| `email` | string | Contact email address |
| `name` | string or null | Display name if known |
| `email_count` | number | Total emails exchanged |
| `last_contact` | number | Unix timestamp of most recent email |

---

## Limitations

- The contact email must be URL-encoded in the path. For example, `alice+tags@example.com` becomes `alice%2Btags%40example.com`.
- Topic analysis is performed on first request and cached for one hour. If your inbox has new emails with that contact since the last analysis, they will not be reflected until the cache expires.
- AI must be configured and enabled for topic analysis. The `/api/contacts/top` endpoint is always available without AI.
- Contacts with very few emails (fewer than 3–5) may produce fewer or less accurate topics.
- Topics are derived from email content only. Attachments are not analyzed.

---

## Related

- [Response Times (feat-016)](feat-016-response-times.md)
- [AI Configuration](../concepts/ai-configuration.md)
- [Keyword Search](../concepts/search.md)
