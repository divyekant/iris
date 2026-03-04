---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Agent API

Iris exposes a REST API for external AI agents and scripts to search, read, draft, and send emails. The Agent API uses Bearer token authentication with granular permission levels and full audit logging.

## Overview

The Agent API lets you build integrations and automations on top of your email. Use cases include:

- An AI assistant that monitors your inbox and drafts responses
- A script that searches for specific emails and extracts data
- A workflow automation that sends reports or notifications from your account

All agent actions are recorded in an audit log that you can review in the Iris UI.

## Creating an API Key

1. Go to **Settings > API Keys**.
2. Click **Create New Key**.
3. Enter a name for the key (e.g., "My AI Agent").
4. Select a permission level (see below).
5. Optionally scope the key to a specific email account.
6. Click **Create**.

The API key is displayed once. Copy it immediately -- you will not be able to see it again. Keys start with `iris_` followed by 32 hex characters.

## Permission Levels

Each API key has one of four permission levels. Higher levels include all permissions of lower levels.

| Level | Allowed Actions | Use Case |
|---|---|---|
| **read_only** | Search, read messages and threads | Monitoring, data extraction |
| **draft_only** | + Create drafts | AI assistants that prepare responses for your review |
| **send_with_approval** | + Send emails | Automated email workflows |
| **autonomous** | + Execute actions, configure settings | Full automation agents |

## Account Scoping

When creating an API key, you can optionally scope it to a specific email account. A scoped key can only access messages and send from that one account. An unscoped key can access all accounts.

## Authentication

All Agent API requests require a Bearer token in the `Authorization` header:

```
Authorization: Bearer iris_your_api_key_here
```

## Endpoints

Base URL: `http://localhost:3000/api/agent`

### Search Messages

Search your email archive by keyword.

```
GET /api/agent/search?q={query}&limit={n}&offset={n}
```

| Parameter | Type | Default | Description |
|---|---|---|---|
| `q` | string | required | Search query |
| `limit` | integer | 50 | Maximum results |
| `offset` | integer | 0 | Pagination offset |

**Required permission:** `read_only`

**Example:**

```bash
curl -H "Authorization: Bearer iris_your_key" \
  "http://localhost:3000/api/agent/search?q=invoice&limit=10"
```

**Response (200):**

```json
{
  "results": [
    {
      "id": "msg-abc-123",
      "account_id": "acct-xyz",
      "thread_id": "thread-456",
      "from_address": "billing@example.com",
      "from_name": "Billing",
      "subject": "Invoice #1234",
      "snippet": "...your <mark>invoice</mark> for January...",
      "date": 1709500800,
      "is_read": true,
      "has_attachments": true
    }
  ],
  "total": 1,
  "query": "invoice"
}
```

### Get Message

Retrieve the full content of a single message.

```
GET /api/agent/messages/{id}
```

**Required permission:** `read_only`

**Example:**

```bash
curl -H "Authorization: Bearer iris_your_key" \
  "http://localhost:3000/api/agent/messages/msg-abc-123"
```

**Response (200):**

```json
{
  "id": "msg-abc-123",
  "message_id": "<unique@mail.example.com>",
  "account_id": "acct-xyz",
  "thread_id": "thread-456",
  "folder": "INBOX",
  "from_address": "alice@example.com",
  "from_name": "Alice",
  "to_addresses": "[\"you@example.com\"]",
  "cc_addresses": null,
  "subject": "Project Update",
  "snippet": "The project is on track...",
  "date": 1709500800,
  "body_text": "The project is on track. We shipped the feature last Friday.",
  "body_html": "<p>The project is on track...</p>",
  "is_read": true,
  "is_starred": false,
  "has_attachments": false,
  "attachments": [],
  "ai_intent": "INFORMATIONAL",
  "ai_priority_score": null,
  "ai_priority_label": "normal",
  "ai_category": "Primary",
  "ai_summary": "Alice reports the project is on track and a feature shipped last Friday."
}
```

### Get Thread

Retrieve all messages in a thread.

```
GET /api/agent/threads/{id}
```

**Required permission:** `read_only`

**Example:**

```bash
curl -H "Authorization: Bearer iris_your_key" \
  "http://localhost:3000/api/agent/threads/thread-456"
```

**Response (200):** An array of message objects (same structure as Get Message), sorted chronologically.

### Create Draft

Create a new email draft.

```
POST /api/agent/drafts
```

**Required permission:** `draft_only`

**Request body:**

```json
{
  "account_id": "acct-xyz",
  "to": ["recipient@example.com"],
  "subject": "Follow-up on Q4 Report",
  "body_text": "Hi, I wanted to follow up on the Q4 report we discussed..."
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `account_id` | string | yes | The account to create the draft for |
| `to` | string[] | no | Recipient email addresses |
| `subject` | string | no | Email subject line |
| `body_text` | string | yes | Plain text body |

**Example:**

```bash
curl -X POST -H "Authorization: Bearer iris_your_key" \
  -H "Content-Type: application/json" \
  -d '{"account_id":"acct-xyz","to":["alice@example.com"],"subject":"Follow-up","body_text":"Hi Alice, just checking in on the proposal."}' \
  "http://localhost:3000/api/agent/drafts"
```

**Response (200):**

```json
{
  "draft_id": "draft-789"
}
```

### Send Email

Send an email immediately.

```
POST /api/agent/send
```

**Required permission:** `send_with_approval`

**Request body:**

```json
{
  "account_id": "acct-xyz",
  "to": ["recipient@example.com"],
  "cc": [],
  "bcc": [],
  "subject": "Monthly Report",
  "body_text": "Please find the monthly report attached.",
  "body_html": "<p>Please find the monthly report attached.</p>",
  "in_reply_to": null,
  "references": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `account_id` | string | yes | The account to send from |
| `to` | string[] | yes | Recipient email addresses |
| `cc` | string[] | no | CC recipients (default: []) |
| `bcc` | string[] | no | BCC recipients (default: []) |
| `subject` | string | yes | Email subject line |
| `body_text` | string | yes | Plain text body |
| `body_html` | string | no | HTML body (optional) |
| `in_reply_to` | string | no | Message-ID being replied to |
| `references` | string | no | References header for threading |

**Example:**

```bash
curl -X POST -H "Authorization: Bearer iris_your_key" \
  -H "Content-Type: application/json" \
  -d '{"account_id":"acct-xyz","to":["alice@example.com"],"subject":"Status Update","body_text":"Everything is on track."}' \
  "http://localhost:3000/api/agent/send"
```

**Response (200):**

```json
{
  "message_id": "msg-sent-001"
}
```

## Error Responses

| Status Code | Meaning |
|---|---|
| 200 | Success |
| 400 | Bad request (missing or invalid fields) |
| 401 | Missing or invalid API key |
| 403 | API key lacks the required permission, or the requested resource is outside the key's account scope |
| 404 | Message, thread, or account not found |
| 500 | Internal server error |
| 502 | SMTP send failure or token refresh failure |

Error responses include a JSON body:

```json
{
  "error": "insufficient permissions"
}
```

## Audit Logging

Every Agent API request is logged in the audit log, including:

- Which API key was used
- What action was performed (search, read, draft, send)
- The resource type and ID
- Whether the request succeeded or was denied
- Timestamp

You can review the audit log in **Settings > Audit Log**, or query it via the API:

```bash
curl -H "X-Session-Token: your_session_token" \
  "http://localhost:3000/api/audit-log?limit=50"
```

## Revoking API Keys

To revoke an API key:

1. Go to **Settings > API Keys**.
2. Find the key you want to revoke.
3. Click **Revoke**.

Revoked keys are immediately invalid. Any requests using a revoked key will receive a 401 response.
