---
id: feat-024
type: feature-doc
audience: external
topic: agent-platform
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Agent Platform

## Overview

Iris v0.4.0 opens the entire API to agents. Your API keys now authenticate against all 200+ routes -- the same endpoints the web UI uses -- not just the original five. Combined with new reply/forward endpoints and per-key rate limiting, agents can do everything a human user can do through the Iris interface.

## Getting Started

If you already have an API key, it works on every route immediately. No migration needed.

```bash
# Before v0.4.0 — agents were limited to /api/agent/*
curl -H "Authorization: Bearer YOUR_KEY" "http://localhost:3000/api/agent/search?q=invoice"

# After v0.4.0 — agents access any route
curl -H "Authorization: Bearer YOUR_KEY" "http://localhost:3000/api/search?q=invoice&semantic=true"
curl -H "Authorization: Bearer YOUR_KEY" "http://localhost:3000/api/analytics/overview"
curl -H "Authorization: Bearer YOUR_KEY" "http://localhost:3000/api/contacts/profiles"
```

The `/api/agent/*` endpoints still work as before. Existing integrations are not affected.

## How It Works

### Unified Authentication

Every protected route now accepts two forms of authentication:

1. **API key** -- `Authorization: Bearer iris_...` header. The key is hashed with SHA-256 and looked up in the database. If valid and not revoked, the request proceeds with the key's permission level.
2. **Session token** -- `X-Session-Token` header or `iris_session` cookie. Used by the web UI. Session auth has full access with no permission restrictions.

If both are present, the Bearer token takes precedence (no CSRF check needed since the caller is explicitly providing credentials).

### Permission Levels

Each API key carries one of four permission levels. Higher levels include all permissions below them.

| Level | What you can do | Example operations |
|---|---|---|
| `read_only` | Read data, search, view analytics | List messages, get threads, search, contacts, analytics |
| `draft_only` | read_only + non-send mutations | Save drafts, archive, label, snooze, create notes |
| `send_with_approval` | draft_only + sending | Send email, reply, forward, cancel send |
| `autonomous` | Full access including config | Change settings, manage webhooks, delegation playbooks |

Some GET endpoints require elevated permissions because they expose sensitive data:

| Endpoint | Required level |
|---|---|
| `GET /api/config`, `GET /api/config/ai` | `autonomous` |
| `GET /api/api-keys` | `autonomous` |
| `GET /api/audit-log` | `autonomous` |
| `GET /api/webhooks` | `draft_only` |

If a request requires a permission the key does not have, the API returns `403 Forbidden`.

### Account Scoping

API keys can optionally be scoped to a single email account. When scoped:

- Query parameters like `account_id` are automatically overridden with the scoped account
- Cross-account listing endpoints are filtered to the scoped account only
- Accessing a message or thread from a different account returns `403 Forbidden`

Unscoped keys have access to all connected accounts.

### Per-Key Rate Limiting

Each API key gets its own rate limit bucket: 500 burst requests, sustained at 5 requests per second. One agent hitting its limit does not affect other agents or the web UI.

## Examples

### Reply to an email

The new reply endpoint handles threading headers, quoted body, and recipient resolution for you. You just provide the message ID and your reply text.

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

The server automatically:
- Sets the `In-Reply-To` header from the original message
- Builds the `References` chain for proper threading
- Adds the `Re:` subject prefix (if not already present)
- Includes the quoted original message in the body
- Sends from the same account that received the original

**Required permission:** `send_with_approval`

### Forward an email

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

**Required permission:** `send_with_approval`

### Draft a reply (for human review)

If you want to prepare a reply without sending it, use the draft endpoints instead:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "body": "Thanks for the update. I have a few questions about the timeline.",
    "reply_all": true
  }' \
  "http://localhost:3000/api/drafts/reply"
```

The draft appears in the Iris UI for review and sending.

**Required permission:** `draft_only`

### Draft a forward

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "to": ["team@example.com"],
    "body": "Sharing this for visibility."
  }' \
  "http://localhost:3000/api/drafts/forward"
```

**Required permission:** `draft_only`

### Access analytics

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/analytics/overview"
```

**Required permission:** `read_only`

### Use semantic search with date filtering

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=budget+concerns&semantic=true&since=2026-01-01&until=2026-03-31"
```

**Required permission:** `read_only`

### MCP access with API keys

MCP sessions now accept API key authentication. The session inherits the key's permission level, and individual tool calls are permission-checked accordingly.

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"method": "initialize"}' \
  "http://localhost:3000/api/mcp/initialize"
```

MCP tool permission mapping:

| Permission level | Allowed tools |
|---|---|
| `read_only` | search_emails, read_email, list_inbox, list_threads, get_thread, get_thread_summary, get_contact_profile, get_inbox_stats, extract_tasks, extract_deadlines |
| `draft_only` | All read_only tools + create_draft, manage_draft, archive_email, star_email, bulk_action |
| `send_with_approval` | All draft_only tools + send_email, chat (when proposing sends) |
| `autonomous` | All tools, no restrictions |

## Configuration

No new configuration is required. The unified auth middleware is active by default in v0.4.0.

| Setting | Description |
|---|---|
| API key creation | **Settings > API Keys** in the Iris UI, or `POST /api/api-keys` with session auth |
| Rate limits | 500 burst / 5 req/sec per API key (not configurable) |
| Account scoping | Set `account_id` when creating a key to restrict access |

## FAQ

**Do I need to update my existing agent scripts?**
No. The `/api/agent/*` endpoints still work exactly as before. You can optionally switch to the direct routes (e.g., `/api/search` instead of `/api/agent/search`) to access additional features like semantic search parameters.

**Can an API key change server settings?**
Only keys with `autonomous` permission can access configuration endpoints. Use the lowest permission level your agent actually needs.

**What happens if my key's rate limit is exceeded?**
The API returns `429 Too Many Requests`. Each key has its own bucket, so other keys and the web UI are unaffected.

**Is the audit log updated for all routes?**
Yes. Every request authenticated with an API key is logged to the audit trail, regardless of which route is called.

**Can I use reply/forward from the old /api/agent/* namespace?**
The reply and forward endpoints are available at `/api/reply`, `/api/forward`, `/api/drafts/reply`, and `/api/drafts/forward`. They are not nested under `/api/agent/` but are accessible with API key auth through unified authentication.
