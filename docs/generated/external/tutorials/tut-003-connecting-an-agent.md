---
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Tutorial: Connecting an AI Agent

This tutorial shows you how to create an API key, authenticate with the Iris API, and make programmatic requests to search, read, reply, forward, and send emails. By the end, you will have a working integration that can interact with your email through the full Iris API.

**Time required:** About 10 minutes.

**Prerequisites:**
- Iris is running with at least one email account connected and some emails synced
- `curl` or any HTTP client

## Step 1: Get Your Session Token

Before you can create an API key, you need your session token (used for the internal management API). You can retrieve it from the browser or from a file.

**Option A: From the browser (recommended)**

Open your browser's developer console on the Iris page and run:

```javascript
fetch('/api/auth/bootstrap').then(r => r.json()).then(d => console.log(d.token))
```

Copy the token that appears.

**Option B: From a file (Docker/scripts)**

If you have `SESSION_TOKEN_FILE` configured in your `.env`:

```bash
export SESSION_TOKEN=$(cat /path/to/session-token)
```

For the rest of this tutorial, replace `YOUR_SESSION_TOKEN` with your actual token.

## Step 2: Create an API Key

Create an API key using the management API. Start with `read_only` permission for safety:

```bash
curl -s -X POST \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "My First Agent", "permission": "read_only"}' \
  "http://localhost:3000/api/api-keys"
```

Response:

```json
{
  "key": "iris_a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "My First Agent",
  "permission": "read_only",
  "key_prefix": "iris_a1b2c3d4"
}
```

**Important:** Copy the `key` value immediately. It is only shown once. You cannot retrieve it later.

For the rest of this tutorial, replace `YOUR_API_KEY` with your actual key.

## Step 3: Search for Emails

Your API key now works on all 200+ routes, not just the agent-specific endpoints. You can use the full search API with operators and semantic search:

**Keyword search:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/search?q=meeting&limit=5" | python3 -m json.tool
```

**Semantic search (meaning-based):**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/search?q=budget+concerns&semantic=true" | python3 -m json.tool
```

**Semantic search with date range:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/search?q=project+deadline&semantic=true&since=2026-01-01&until=2026-03-31" | python3 -m json.tool
```

Response:

```json
{
  "results": [
    {
      "id": "msg-abc-123",
      "account_id": "acct-xyz",
      "thread_id": "thread-456",
      "from_address": "alice@example.com",
      "from_name": "Alice",
      "subject": "Team meeting tomorrow at 3pm",
      "snippet": "...let's discuss the <mark>meeting</mark> agenda...",
      "date": 1709500800,
      "is_read": true,
      "has_attachments": false
    }
  ],
  "total": 1,
  "query": "meeting"
}
```

## Step 4: Read a Full Message

Take a message ID from the search results and fetch its full content:

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/messages/msg-abc-123" | python3 -m json.tool
```

Response includes the full body text, HTML, AI metadata, and more:

```json
{
  "id": "msg-abc-123",
  "subject": "Team meeting tomorrow at 3pm",
  "body_text": "Hi team,\n\nLet's discuss the meeting agenda for tomorrow...",
  "ai_intent": "ACTION_REQUEST",
  "ai_priority_label": "high",
  "ai_category": "Primary",
  "..."
}
```

## Step 5: Read an Entire Thread

If the message is part of a conversation, fetch the full thread:

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/threads/thread-456" | python3 -m json.tool
```

Response is an array of messages in chronological order.

## Step 6: Reply to an Email

To reply to emails, you need a key with `send_with_approval` permission. Create one:

```bash
curl -s -X POST \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Reply Agent", "permission": "send_with_approval"}' \
  "http://localhost:3000/api/api-keys"
```

Now reply to a message. The server handles threading headers, quoted body, and recipients automatically:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_SEND_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "body": "Thanks for the agenda. I will prepare the Q1 numbers for the meeting.",
    "reply_all": false
  }' \
  "http://localhost:3000/api/reply"
```

Response:

```json
{
  "message_id": "msg-sent-001"
}
```

To reply to all recipients, set `"reply_all": true`. The server automatically includes all original To and CC recipients (minus yourself).

## Step 7: Forward an Email

Forward an email to someone else:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_SEND_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "to": ["colleague@example.com"],
    "body": "FYI - see the meeting agenda below."
  }' \
  "http://localhost:3000/api/forward"
```

The server adds the `Fwd:` prefix, includes the original message body, and sends from the same account that received it.

## Step 8: Create a Draft (for Human Review)

If you want to prepare replies without sending them, use the draft endpoints. You need `draft_only` permission or higher:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_SEND_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "message_id": "msg-abc-123",
    "body": "Thanks for sharing. I have a few questions about the timeline.",
    "reply_all": true
  }' \
  "http://localhost:3000/api/drafts/reply"
```

Response:

```json
{
  "draft_id": "draft-789"
}
```

The draft now appears in the Iris UI under your drafts, where you can review and send it.

## Step 9: Access the Full API

With unified auth, your API key works on every protected route. Here are some examples:

**Get analytics overview:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/analytics/overview" | python3 -m json.tool
```

**List contact profiles:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/contacts/profiles" | python3 -m json.tool
```

**Query the knowledge graph:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/graph?query=Sarah" | python3 -m json.tool
```

**Check inbox stats:**

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/ai/inbox-stats" | python3 -m json.tool
```

## Step 10: Review the Audit Log

Every action your agent takes is logged. Review the audit log (requires `autonomous` permission):

```bash
curl -s -H "Authorization: Bearer YOUR_ADMIN_KEY" \
  "http://localhost:3000/api/audit-log?limit=10" | python3 -m json.tool
```

Response:

```json
[
  {
    "id": 1,
    "api_key_id": "550e8400-...",
    "key_name": "My First Agent",
    "action": "search",
    "resource_type": "message",
    "resource_id": null,
    "details": "q=meeting, results=1",
    "status": "success",
    "created_at": 1709500900
  }
]
```

## Step 11: Scope a Key to a Specific Account (Optional)

If you want to restrict an API key to only access one email account, pass the `account_id` when creating the key:

```bash
curl -s -X POST \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Work Account Agent",
    "permission": "read_only",
    "account_id": "acct-xyz"
  }' \
  "http://localhost:3000/api/api-keys"
```

This key can only search and read messages from account `acct-xyz`. Attempting to access messages from other accounts returns `403 Forbidden`.

## Step 12: Revoke a Key When Done

When you no longer need an API key, revoke it:

```bash
curl -s -X DELETE \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/api-keys/550e8400-e29b-41d4-a716-446655440000"
```

Returns `204 No Content` on success. The revoked key is immediately invalid.

## Building an Integration

Now that you know the API, here are some ideas for what to build:

- **Daily briefing script** -- search for unread emails each morning and generate a summary using AI inbox stats
- **Auto-responder** -- search for emails matching a pattern, use `/api/drafts/reply` to draft a response, review in Iris, then send
- **Data extraction** -- search for invoices or receipts and extract amounts and dates from the body text
- **Monitoring** -- periodically search for emails from important contacts and send alerts
- **Semantic research** -- use `semantic=true` with date ranges to find all discussion around a topic in a specific time period
- **Knowledge graph queries** -- find all emails related to a person or project via `/api/graph`

See the [API Reference](../api-reference.md) for complete endpoint documentation and the [Cookbook](../cookbook.md) for more recipes.

## Troubleshooting

**401 Unauthorized:**
Check that you are using the correct API key in the `Authorization: Bearer` header. Keys start with `iris_`.

**403 Forbidden ("insufficient permissions"):**
Your key does not have the required permission level. Create a new key with a higher permission. For example, replying requires `send_with_approval`, and accessing the audit log requires `autonomous`.

**403 Forbidden ("message not in scope"):**
Your key is scoped to a specific account, and you are trying to access a message from a different account.

**404 Not Found on reply/forward:**
The `message_id` you specified does not exist in the database. Use search to find valid message IDs.

**429 Too Many Requests:**
Your API key has exceeded its rate limit (500 burst, 5/sec sustained). Each key has its own bucket. Wait a moment and retry.

**502 Bad Gateway on send:**
The SMTP connection failed. Check that the email account has valid OAuth tokens. Try removing and re-adding the account in the Iris UI.
