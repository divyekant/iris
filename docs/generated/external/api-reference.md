---
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# API Reference

The Iris API lets you search, read, compose, reply, forward, and manage emails programmatically. All protected endpoints accept either a session token (used by the web UI) or a Bearer API key (used by agents and scripts).

## Authentication

### API Key (recommended for agents)

Include your API key in the `Authorization` header:

```
Authorization: Bearer iris_your_api_key_here
```

API keys are created in **Settings > API Keys** within the Iris web interface, or via `POST /api/api-keys` with session auth. Keys start with `iris_` followed by 32 hex characters (37 characters total).

API keys work on **all protected routes** -- the same endpoints the web UI uses. If the key is missing, invalid, or revoked, the API returns `401 Unauthorized`.

### Session Token (used by the web UI)

Include the session token in the `X-Session-Token` header or `iris_session` cookie:

```
X-Session-Token: your_session_token
```

Session auth has full access with no permission restrictions.

### Auth Precedence

If both a Bearer token and a session token are present, the Bearer token takes precedence. No CSRF check is performed when using Bearer auth.

## Base URL

```
http://localhost:3000/api
```

Replace `localhost:3000` with your Iris server address if different.

## Permission Levels

Each API key is assigned one of four permission levels. Higher levels include all permissions of lower levels.

| Level | Actions | Description |
|---|---|---|
| `read_only` | `read`, `search` | Search and read messages, threads, contacts, analytics |
| `draft_only` | `read`, `search`, `draft` | All of the above, plus create drafts, archive, label, snooze |
| `send_with_approval` | `read`, `search`, `draft`, `send` | All of the above, plus send, reply, forward emails |
| `autonomous` | `read`, `search`, `draft`, `send`, `execute`, `configure` | Full access including config and admin |

**Elevated permission requirements for specific endpoints:**

| Endpoint | Required level | Reason |
|---|---|---|
| `GET /api/config`, `GET /api/config/ai` | `autonomous` | Exposes server configuration |
| `GET /api/api-keys` | `autonomous` | Enumerates API keys |
| `GET /api/audit-log` | `autonomous` | Audit trail data |
| `GET /api/webhooks` | `draft_only` | Webhook URLs are operational data |

If a request requires a permission the key does not have, the API returns `403 Forbidden`.

## Account Scoping

API keys can optionally be scoped to a specific email account. A scoped key can only:

- Search messages belonging to that account
- Read messages belonging to that account
- Create drafts for that account
- Send emails from that account
- Access any endpoint, filtered to that account

Attempting to access resources outside the scoped account returns `403 Forbidden`.

---

## Endpoints

### Search Messages

Search your email archive by keyword or meaning.

```
GET /api/search
```

**Query Parameters:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `q` | string | yes | -- | Search query text. Supports operators: `from:`, `to:`, `subject:`, `is:unread/read/starred`, `has:attachment`, `before:YYYY-MM-DD`, `after:YYYY-MM-DD`, `category:` |
| `limit` | integer | no | 50 | Maximum number of results (max 500) |
| `offset` | integer | no | 0 | Pagination offset |
| `semantic` | boolean | no | false | Use semantic (meaning-based) search via Memories vector store |
| `since` | string | no | -- | Start date for temporal search (ISO 8601, e.g., `2026-01-01`). Only used when `semantic=true`. |
| `until` | string | no | -- | End date for temporal search (ISO 8601). Only used when `semantic=true`. |
| `account_id` | string | no | -- | Filter results to a specific email account |
| `has_attachment` | boolean | no | -- | Filter to messages with attachments |
| `after` | integer | no | -- | Unix timestamp: only messages after this date |
| `before` | integer | no | -- | Unix timestamp: only messages before this date |

**Required Permission:** `read_only`

**Example -- keyword search:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=invoice&limit=5"
```

**Example -- semantic search with date range:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=budget+concerns&semantic=true&since=2026-01-01&until=2026-03-31"
```

**Example -- search operators:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=from:alice+has:attachment+invoice"
```

**Response (200 OK):**

```json
{
  "results": [
    {
      "id": "msg-abc-123",
      "account_id": "acct-xyz",
      "thread_id": "thread-456",
      "from_address": "billing@example.com",
      "from_name": "Billing Department",
      "subject": "Invoice #1234 - January 2026",
      "snippet": "...your <mark>invoice</mark> for January is ready...",
      "date": 1709500800,
      "is_read": true,
      "has_attachments": true
    }
  ],
  "total": 1,
  "query": "invoice",
  "parsed_operators": []
}
```

---

### Get Message

Retrieve the full content and metadata of a single message.

```
GET /api/messages/{id}
```

**Path Parameters:**

| Parameter | Type | Description |
|---|---|---|
| `id` | string | The message ID |

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/messages/msg-abc-123"
```

**Response (200 OK):**

```json
{
  "id": "msg-abc-123",
  "message_id": "<unique-id@mail.example.com>",
  "account_id": "acct-xyz",
  "thread_id": "thread-456",
  "folder": "INBOX",
  "from_address": "alice@example.com",
  "from_name": "Alice Johnson",
  "to_addresses": "[\"you@example.com\"]",
  "cc_addresses": null,
  "subject": "Project Update - Q1 2026",
  "snippet": "The project is on track...",
  "date": 1709500800,
  "body_text": "Hi,\n\nThe project is on track. We shipped the new feature last Friday and user feedback has been positive.\n\nBest,\nAlice",
  "body_html": "<p>Hi,</p><p>The project is on track...</p>",
  "is_read": true,
  "is_starred": false,
  "has_attachments": false,
  "attachments": [],
  "ai_intent": "INFORMATIONAL",
  "ai_priority_score": null,
  "ai_priority_label": "normal",
  "ai_category": "Primary",
  "ai_summary": "Alice reports the project is on track with positive user feedback after shipping a new feature."
}
```

---

### Get Thread

Retrieve all messages in an email thread, sorted chronologically.

```
GET /api/threads/{id}
```

**Path Parameters:**

| Parameter | Type | Description |
|---|---|---|
| `id` | string | The thread ID |

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/threads/thread-456"
```

**Response (200 OK):**

An array of message objects (same structure as Get Message), ordered by date ascending.

---

### Reply to a Message

Send a reply to an existing email. The server handles threading headers, quoted body, recipient resolution, and subject prefix automatically.

```
POST /api/reply
```

**Required Permission:** `send_with_approval`

**Request Body:**

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `message_id` | string | yes | -- | The ID of the message to reply to |
| `body` | string | yes | -- | Your reply text |
| `reply_all` | boolean | no | `false` | If true, reply to all original recipients (To + CC minus yourself) |

**Server handles automatically:**
- `In-Reply-To` header from the original message's Message-ID
- `References` chain for proper email threading
- `Re:` subject prefix (if not already present)
- Quoted original message body
- Account resolution from the original message

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "body": "Thanks, I will review the contract and get back to you by Friday.",
    "reply_all": false
  }' \
  "http://localhost:3000/api/reply"
```

**Response (200 OK):**

```json
{
  "message_id": "msg-sent-001"
}
```

---

### Forward a Message

Forward an existing email to new recipients. The server includes the original message body and applies the subject prefix.

```
POST /api/forward
```

**Required Permission:** `send_with_approval`

**Request Body:**

| Field | Type | Required | Description |
|---|---|---|---|
| `message_id` | string | yes | The ID of the message to forward |
| `to` | string[] | yes | Recipients to forward to |
| `body` | string | no | Optional message to include above the forwarded content |

**Server handles automatically:**
- `Fwd:` subject prefix (if not already present)
- Original message body included below a separator
- Account resolution from the original message

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "to": ["colleague@example.com"],
    "body": "FYI - see the thread below."
  }' \
  "http://localhost:3000/api/forward"
```

**Response (200 OK):**

```json
{
  "message_id": "msg-sent-002"
}
```

---

### Draft a Reply

Create a reply draft for human review instead of sending immediately.

```
POST /api/drafts/reply
```

**Required Permission:** `draft_only`

**Request Body:** Same as Reply to a Message.

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "body": "I have a few questions about the timeline.",
    "reply_all": true
  }' \
  "http://localhost:3000/api/drafts/reply"
```

**Response (200 OK):**

```json
{
  "draft_id": "draft-abc-789"
}
```

---

### Draft a Forward

Create a forward draft for human review instead of sending immediately.

```
POST /api/drafts/forward
```

**Required Permission:** `draft_only`

**Request Body:** Same as Forward a Message.

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "to": ["team@example.com"],
    "body": "Sharing for visibility."
  }' \
  "http://localhost:3000/api/drafts/forward"
```

**Response (200 OK):**

```json
{
  "draft_id": "draft-fwd-456"
}
```

---

### Create Draft

Create a new email draft from scratch.

```
POST /api/drafts
```

**Required Permission:** `draft_only`

**Request Body:**

| Field | Type | Required | Description |
|---|---|---|---|
| `account_id` | string | yes | The email account ID to create the draft for |
| `to` | string[] | no | Recipient email addresses |
| `subject` | string | no | Email subject line |
| `body_text` | string | yes | Plain text email body |

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["alice@example.com"],
    "subject": "Follow-up: Q1 Planning",
    "body_text": "Hi Alice,\n\nJust following up on our discussion about Q1 priorities.\n\nThanks"
  }' \
  "http://localhost:3000/api/drafts"
```

**Response (200 OK):**

```json
{
  "draft_id": "draft-abc-789"
}
```

---

### Send Email

Send an email immediately from a connected account.

```
POST /api/send
```

**Required Permission:** `send_with_approval`

**Request Body:**

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `account_id` | string | yes | -- | The email account ID to send from |
| `to` | string[] | yes | -- | Recipient email addresses |
| `cc` | string[] | no | `[]` | CC recipients |
| `bcc` | string[] | no | `[]` | BCC recipients |
| `subject` | string | yes | -- | Email subject line |
| `body_text` | string | yes | -- | Plain text email body |
| `body_html` | string | no | null | HTML email body |
| `in_reply_to` | string | no | null | Message-ID for threading (prefer `/api/reply` instead) |
| `references` | string | no | null | References header for threading |

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["alice@example.com"],
    "subject": "Monthly Status Report",
    "body_text": "Hi team,\n\nAll milestones are on track. No blockers.\n\nBest regards"
  }' \
  "http://localhost:3000/api/send"
```

**Response (200 OK):**

```json
{
  "message_id": "msg-sent-001"
}
```

---

### Legacy Agent Endpoints

The original `/api/agent/*` endpoints still work for backwards compatibility:

| Legacy endpoint | Equivalent |
|---|---|
| `GET /api/agent/search` | `GET /api/search` |
| `GET /api/agent/messages/{id}` | `GET /api/messages/{id}` |
| `GET /api/agent/threads/{id}` | `GET /api/threads/{id}` |
| `POST /api/agent/drafts` | `POST /api/drafts` |
| `POST /api/agent/send` | `POST /api/send` |

You can migrate to the direct routes at any pace. Both paths authenticate the same way and return the same responses.

---

### Queue Status

Check the current state of the background job queue.

```
GET /api/ai/queue-status
```

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/ai/queue-status"
```

**Response (200 OK):**

```json
{
  "pending": 3,
  "processing": 1,
  "failed": 0,
  "done_today": 142
}
```

---

### Chat Memory

Retrieve the AI chat assistant's stored session summaries and learned preferences.

```
GET /api/ai/chat/memory
```

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/ai/chat/memory"
```

**Response (200 OK):**

```json
{
  "summaries": [
    {
      "session_id": "sess-abc-123",
      "summary": "User asked about emails from the marketing team regarding the Q1 campaign.",
      "created_at": "2026-03-02T14:30:00Z"
    }
  ],
  "preferences": [
    "Prefers concise email summaries over detailed ones"
  ]
}
```

---

## Status Codes

| Code | Meaning |
|---|---|
| `200 OK` | Request succeeded |
| `201 Created` | Resource created (used by API key creation) |
| `204 No Content` | Resource deleted (used by API key revocation) |
| `400 Bad Request` | Invalid request body, missing required fields, or invalid email format |
| `401 Unauthorized` | Missing `Authorization` header, invalid API key, or revoked key |
| `403 Forbidden` | API key lacks required permission, or resource is outside account scope |
| `404 Not Found` | Message, thread, or account not found |
| `429 Too Many Requests` | Rate limit exceeded for this API key |
| `500 Internal Server Error` | Database error or unexpected server failure |
| `502 Bad Gateway` | SMTP send failure or OAuth token refresh failure |

All error responses include a JSON body with an `error` field:

```json
{
  "error": "description of the problem"
}
```

---

## Rate Limits

Each API key gets its own rate limit bucket:

| Parameter | Value |
|---|---|
| Burst capacity | 500 requests |
| Sustained rate | 5 requests/second |
| Scope | Per API key (keys do not share buckets) |

Session auth (web UI) has a separate rate limit: 500 requests/minute per session token.

Auth endpoints (`/auth/bootstrap`, `/auth/login`, `/auth/oauth/*`) have a stricter limit: 10 burst, 1 request/second.

When rate-limited, the API returns `429 Too Many Requests`.

---

## Audit Logging

Every API key request is recorded in the audit log with:

- The API key ID and name
- The action performed
- The resource type and resource ID
- Additional details (search query, recipient addresses)
- Status (`success`, `forbidden`, `error`)
- Timestamp

You can query the audit log (requires `autonomous` permission):

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/audit-log?limit=20&offset=0"
```

Optional query parameters:

| Parameter | Type | Description |
|---|---|---|
| `api_key_id` | string | Filter to a specific API key |
| `limit` | integer | Max entries to return (default: 50) |
| `offset` | integer | Pagination offset (default: 0) |
