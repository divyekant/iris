---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Agent API Reference

The Iris Agent API allows external AI agents, scripts, and integrations to search, read, draft, and send emails programmatically. All endpoints require Bearer token authentication.

## Authentication

Every request to the Agent API must include an API key in the `Authorization` header:

```
Authorization: Bearer iris_your_api_key_here
```

API keys are created in **Settings > API Keys** within the Iris web interface. Keys start with `iris_` followed by 32 hex characters (37 characters total).

If the key is missing, invalid, or revoked, the API returns `401 Unauthorized`.

## Base URL

```
http://localhost:3000/api/agent
```

Replace `localhost:3000` with your Iris server address if different.

## Permission Levels

Each API key is assigned one of four permission levels. Higher levels include all permissions of lower levels.

| Level | Actions | Description |
|---|---|---|
| `read_only` | `read`, `search` | Search and read messages and threads |
| `draft_only` | `read`, `search`, `draft` | All of the above, plus create drafts |
| `send_with_approval` | `read`, `search`, `draft`, `send` | All of the above, plus send emails |
| `autonomous` | `read`, `search`, `draft`, `send`, `execute`, `configure` | Full access |

If a request requires a permission the key does not have, the API returns `403 Forbidden`.

## Account Scoping

API keys can optionally be scoped to a specific email account. A scoped key can only:

- Search messages belonging to that account
- Read messages belonging to that account
- Create drafts for that account
- Send emails from that account

Attempting to access resources outside the scoped account returns `403 Forbidden` with `"error": "message not in scope"` or similar.

---

## Endpoints

### Search Messages

Search your email archive by keyword using full-text search.

```
GET /api/agent/search
```

**Query Parameters:**

| Parameter | Type | Required | Default | Description |
|---|---|---|---|---|
| `q` | string | yes | -- | Search query text |
| `limit` | integer | no | 50 | Maximum number of results |
| `offset` | integer | no | 0 | Pagination offset |

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  "http://localhost:3000/api/agent/search?q=invoice&limit=5"
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
  "query": "invoice"
}
```

**Empty query returns an empty result set:**

```bash
curl -s -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  "http://localhost:3000/api/agent/search?q="
```

```json
{
  "results": [],
  "total": 0,
  "query": ""
}
```

---

### Get Message

Retrieve the full content and metadata of a single message.

```
GET /api/agent/messages/{id}
```

**Path Parameters:**

| Parameter | Type | Description |
|---|---|---|
| `id` | string | The message ID |

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  "http://localhost:3000/api/agent/messages/msg-abc-123"
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

**Error (404 Not Found):**

```json
{
  "error": "message not found"
}
```

---

### Get Thread

Retrieve all messages in an email thread, sorted chronologically.

```
GET /api/agent/threads/{id}
```

**Path Parameters:**

| Parameter | Type | Description |
|---|---|---|
| `id` | string | The thread ID |

**Required Permission:** `read_only`

**Example Request:**

```bash
curl -s -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  "http://localhost:3000/api/agent/threads/thread-456"
```

**Response (200 OK):**

An array of message objects (same structure as Get Message), ordered by date ascending:

```json
[
  {
    "id": "msg-001",
    "message_id": "<first@mail.example.com>",
    "account_id": "acct-xyz",
    "thread_id": "thread-456",
    "folder": "INBOX",
    "from_address": "bob@example.com",
    "from_name": "Bob",
    "subject": "Project kickoff",
    "date": 1709400000,
    "body_text": "Let's get started on the new project...",
    "is_read": true,
    "..."
  },
  {
    "id": "msg-002",
    "message_id": "<reply@mail.example.com>",
    "account_id": "acct-xyz",
    "thread_id": "thread-456",
    "folder": "INBOX",
    "from_address": "alice@example.com",
    "from_name": "Alice",
    "subject": "Re: Project kickoff",
    "date": 1709450000,
    "body_text": "Sounds good, I'll prepare the timeline...",
    "is_read": true,
    "..."
  }
]
```

---

### Create Draft

Create a new email draft that you can review and send later from the Iris UI.

```
POST /api/agent/drafts
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
  -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["alice@example.com"],
    "subject": "Follow-up: Q1 Planning",
    "body_text": "Hi Alice,\n\nJust following up on our discussion about Q1 priorities. Could you share the updated timeline?\n\nThanks"
  }' \
  "http://localhost:3000/api/agent/drafts"
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
POST /api/agent/send
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
| `in_reply_to` | string | no | null | Message-ID of the email being replied to (for threading) |
| `references` | string | no | null | References header value (for threading) |

**Example Request:**

```bash
curl -s -X POST \
  -H "Authorization: Bearer iris_abc123def456abc123def456abc123de" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["alice@example.com"],
    "cc": ["bob@example.com"],
    "subject": "Monthly Status Report",
    "body_text": "Hi team,\n\nPlease find the monthly status report below.\n\nAll milestones are on track. No blockers.\n\nBest regards",
    "body_html": "<p>Hi team,</p><p>Please find the monthly status report below.</p><p>All milestones are on track. No blockers.</p><p>Best regards</p>"
  }' \
  "http://localhost:3000/api/agent/send"
```

**Response (200 OK):**

```json
{
  "message_id": "msg-sent-001"
}
```

**Error (502 Bad Gateway) -- SMTP failure:**

```json
{
  "error": "Connection refused (os error 111)"
}
```

---

### Queue Status

Check the current state of the background job queue.

```
GET /api/ai/queue-status
```

**Authentication:** Session token (not agent API key)

**Example Request:**

```bash
curl -s -H "X-Session-Token: your_session_token" \
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

| Field | Type | Description |
|---|---|---|
| `pending` | integer | Jobs waiting to be processed |
| `processing` | integer | Jobs currently running |
| `failed` | integer | Jobs that have exceeded retry attempts |
| `done_today` | integer | Jobs successfully completed since midnight |

---

### Chat Memory

Retrieve the AI chat assistant's stored session summaries and learned preferences.

```
GET /api/ai/chat/memory
```

**Authentication:** Session token (not agent API key)

**Example Request:**

```bash
curl -s -H "X-Session-Token: your_session_token" \
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

| Field | Type | Description |
|---|---|---|
| `summaries` | array | Past chat session summaries with session ID, summary text, and timestamp |
| `preferences` | array of strings | User preferences and patterns extracted from chat history |

---

## Status Codes

| Code | Meaning |
|---|---|
| `200 OK` | Request succeeded |
| `201 Created` | Resource created (used by API key creation, not agent endpoints) |
| `400 Bad Request` | Invalid request body, missing required fields, or invalid email format |
| `401 Unauthorized` | Missing `Authorization` header, invalid API key, or revoked key |
| `403 Forbidden` | API key lacks required permission, or resource is outside account scope |
| `404 Not Found` | Message, thread, or account not found |
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

There are no rate limits in the current version. Since Iris is a self-hosted application, you are limited only by your own server resources.

---

## Audit Logging

Every Agent API request is recorded in the audit log with:

- The API key ID and name
- The action performed (`search`, `read`, `draft`, `send`)
- The resource type (`message`, `thread`) and resource ID
- Additional details (search query, recipient addresses)
- Status (`success`, `forbidden`, `error`)
- Timestamp

You can query the audit log via the internal API (requires session authentication, not agent authentication):

```bash
curl -s -H "X-Session-Token: your_session_token" \
  "http://localhost:3000/api/audit-log?limit=20&offset=0"
```

Optional query parameters:

| Parameter | Type | Description |
|---|---|---|
| `api_key_id` | string | Filter to a specific API key |
| `limit` | integer | Max entries to return (default: 50) |
| `offset` | integer | Pagination offset (default: 0) |
