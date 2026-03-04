---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Cookbook

Practical recipes for common tasks using the Iris API. All examples use `curl` and assume your Iris server is running on `http://localhost:3000`.

Replace `YOUR_API_KEY` with an actual agent API key, and `YOUR_SESSION_TOKEN` with your session token where indicated.

---

## Check System Health

Verify that Iris and its dependencies are running correctly.

```bash
curl -s http://localhost:3000/api/health | python3 -m json.tool
```

```json
{
  "status": "ok",
  "version": "0.1.0",
  "db": true,
  "ollama": true,
  "memories": false
}
```

- `status` is `"ok"` when the database is healthy, `"degraded"` otherwise.
- `ollama` and `memories` indicate whether optional AI services are reachable.

No authentication required.

---

## Search for Emails from a Specific Sender

Find all emails from a particular person using the Agent API:

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/agent/search?q=alice%40example.com&limit=20" \
  | python3 -m json.tool
```

You can also search by name:

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/agent/search?q=Alice+Johnson&limit=20" \
  | python3 -m json.tool
```

The `q` parameter is URL-encoded. Use `%40` for `@` and `+` or `%20` for spaces.

---

## Get a Full Thread Conversation

Retrieve all messages in a thread to see the complete conversation:

```bash
# First, find a thread ID from a search result
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/agent/search?q=project+update&limit=1" \
  | python3 -m json.tool

# Then fetch the full thread
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/agent/threads/THREAD_ID_HERE" \
  | python3 -m json.tool
```

The response is an array of messages sorted chronologically, with full body text and AI metadata.

---

## Create and Send an Email via the Agent API

This recipe creates a draft and then sends an email in two separate steps. You need a `send_with_approval` key (or higher) for sending.

**Step 1: Create a draft for review**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_DRAFT_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "YOUR_ACCOUNT_ID",
    "to": ["recipient@example.com"],
    "subject": "Weekly Status Report",
    "body_text": "Hi team,\n\nHere is this week'\''s status update:\n\n- Feature X shipped\n- Bug Y fixed\n- Planning for next sprint\n\nBest regards"
  }' \
  "http://localhost:3000/api/agent/drafts" | python3 -m json.tool
```

**Step 2: Send directly (skipping draft)**

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_SEND_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "YOUR_ACCOUNT_ID",
    "to": ["recipient@example.com"],
    "cc": ["manager@example.com"],
    "subject": "Weekly Status Report",
    "body_text": "Hi team,\n\nHere is this week'\''s status update:\n\n- Feature X shipped\n- Bug Y fixed\n- Planning for next sprint\n\nBest regards",
    "body_html": "<p>Hi team,</p><p>Here is this week'\''s status update:</p><ul><li>Feature X shipped</li><li>Bug Y fixed</li><li>Planning for next sprint</li></ul><p>Best regards</p>"
  }' \
  "http://localhost:3000/api/agent/send" | python3 -m json.tool
```

---

## Search with Semantic Mode

Semantic search finds emails by meaning, not just keywords. Requires the Memories service. Uses the internal API (session auth, not agent auth).

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/search?q=travel+plans+for+next+month&semantic=true&limit=10" \
  | python3 -m json.tool
```

Compare with keyword search:

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/search?q=travel+plans+for+next+month&limit=10" \
  | python3 -m json.tool
```

Semantic search may return results that do not contain the exact words "travel plans" but discuss related concepts like "flight booking" or "hotel reservation."

---

## Batch Mark Messages as Read

Mark multiple messages as read in a single request. Uses the internal API (session auth).

```bash
curl -s -X PATCH \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "ids": ["msg-001", "msg-002", "msg-003"],
    "action": "mark_read"
  }' \
  "http://localhost:3000/api/messages/batch" | python3 -m json.tool
```

```json
{
  "updated": 3
}
```

Available batch actions: `archive`, `delete`, `mark_read`, `mark_unread`, `star`, `unstar`.

You can include up to 1,000 message IDs in a single batch.

---

## Get AI Feedback Statistics

See how often you have corrected the AI and what the most common correction patterns are.

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/ai/feedback-stats" | python3 -m json.tool
```

```json
{
  "total_corrections": 42,
  "by_field": [
    { "field": "category", "count": 25 },
    { "field": "priority_label", "count": 12 },
    { "field": "intent", "count": 5 }
  ],
  "common_corrections": [
    {
      "field": "category",
      "original": "Promotions",
      "corrected": "Updates",
      "count": 8
    },
    {
      "field": "priority_label",
      "original": "low",
      "corrected": "normal",
      "count": 4
    }
  ]
}
```

---

## Correct an AI Classification

If the AI assigned the wrong category to an email, correct it:

```bash
curl -s -X PUT \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"field": "category", "value": "Primary"}' \
  "http://localhost:3000/api/messages/msg-abc-123/ai-feedback" \
  | python3 -m json.tool
```

```json
{
  "updated": true
}
```

Valid fields and values:

| Field | Valid Values |
|---|---|
| `category` | Primary, Updates, Social, Promotions, Finance, Travel, Newsletters |
| `priority_label` | urgent, high, normal, low |
| `intent` | ACTION_REQUEST, INFORMATIONAL, TRANSACTIONAL, SOCIAL, MARKETING, NOTIFICATION |

---

## List All Connected Accounts

See which email accounts are connected:

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/accounts" | python3 -m json.tool
```

The response includes each account's ID (needed for composing, drafting, and sending), email address, provider, and sync status.

---

## List Active API Keys

See all active (non-revoked) API keys:

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/api-keys" | python3 -m json.tool
```

Each key shows its prefix (first 12 characters), permission level, last used time, and creation date. The full key value is never shown after creation.

---

## Review the Agent Audit Log

See what your agent API keys have been doing:

```bash
# All entries
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/audit-log?limit=20" | python3 -m json.tool

# Filtered to a specific API key
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/audit-log?api_key_id=KEY_ID_HERE&limit=20" \
  | python3 -m json.tool
```

---

## Get AI Configuration Status

Check the current AI settings and service connectivity:

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/config/ai" | python3 -m json.tool
```

```json
{
  "ollama_url": "http://localhost:11434",
  "model": "llama3.2",
  "enabled": true,
  "connected": true,
  "memories_url": "http://localhost:8900",
  "memories_connected": false
}
```

---

## Test Ollama Connection and List Models

See which models are available:

```bash
curl -s -X POST -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/config/ai/test" | python3 -m json.tool
```

```json
{
  "connected": true,
  "models": ["llama3.2", "mistral", "codellama"]
}
```
