---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Tutorial: Connecting an AI Agent

This tutorial shows you how to create an API key, authenticate with the Agent API, and make programmatic requests to search, read, draft, and send emails. By the end, you will have a working integration that can interact with your email through the Iris API.

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

Use the agent search endpoint to find emails by keyword:

```bash
curl -s -H "Authorization: Bearer YOUR_API_KEY" \
  "http://localhost:3000/api/agent/search?q=meeting&limit=5" | python3 -m json.tool
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
  "http://localhost:3000/api/agent/messages/msg-abc-123" | python3 -m json.tool
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
  "http://localhost:3000/api/agent/threads/thread-456" | python3 -m json.tool
```

Response is an array of messages in chronological order.

## Step 6: Create a Draft (Requires Higher Permission)

To create drafts, you need a key with `draft_only` permission or higher. Create a new key:

```bash
curl -s -X POST \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Draft Agent", "permission": "draft_only"}' \
  "http://localhost:3000/api/api-keys"
```

Now use the new key to create a draft. You need a valid `account_id` -- get one from the search results or list accounts:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_DRAFT_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["colleague@example.com"],
    "subject": "Re: Team meeting tomorrow at 3pm",
    "body_text": "Hi Alice,\n\nI have reviewed the agenda. Looks good to me.\n\nBest"
  }' \
  "http://localhost:3000/api/agent/drafts"
```

Response:

```json
{
  "draft_id": "draft-789"
}
```

The draft now appears in the Iris UI under your drafts, where you can review and send it.

## Step 7: Send an Email (Requires Higher Permission)

To send emails, you need a key with `send_with_approval` permission or higher:

```bash
curl -s -X POST \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "Send Agent", "permission": "send_with_approval"}' \
  "http://localhost:3000/api/api-keys"
```

Send an email:

```bash
curl -s -X POST \
  -H "Authorization: Bearer YOUR_SEND_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "account_id": "acct-xyz",
    "to": ["recipient@example.com"],
    "subject": "Automated Status Update",
    "body_text": "This is an automated status update sent via the Iris Agent API."
  }' \
  "http://localhost:3000/api/agent/send"
```

Response:

```json
{
  "message_id": "msg-sent-001"
}
```

## Step 8: Review the Audit Log

Every action your agent takes is logged. Review the audit log:

```bash
curl -s -H "X-Session-Token: YOUR_SESSION_TOKEN" \
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

## Step 9: Scope a Key to a Specific Account (Optional)

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

## Step 10: Revoke a Key When Done

When you no longer need an API key, revoke it:

```bash
curl -s -X DELETE \
  -H "X-Session-Token: YOUR_SESSION_TOKEN" \
  "http://localhost:3000/api/api-keys/550e8400-e29b-41d4-a716-446655440000"
```

Returns `204 No Content` on success. The revoked key is immediately invalid.

## Building an Integration

Now that you know the API, here are some ideas for what to build:

- **Daily briefing script** -- search for unread emails each morning and generate a summary
- **Auto-responder** -- search for emails matching a pattern, draft a response, review in Iris, then send
- **Data extraction** -- search for invoices or receipts and extract amounts and dates from the body text
- **Monitoring** -- periodically search for emails from important contacts and send alerts

See the [API Reference](../api-reference.md) for complete endpoint documentation and the [Cookbook](../cookbook.md) for more recipes.

## Troubleshooting

**401 Unauthorized:**
Check that you are using the correct API key in the `Authorization: Bearer` header. Keys start with `iris_`.

**403 Forbidden ("insufficient permissions"):**
Your key does not have the required permission level. Create a new key with a higher permission.

**403 Forbidden ("message not in scope"):**
Your key is scoped to a specific account, and you are trying to access a message from a different account.

**502 Bad Gateway on send:**
The SMTP connection failed. Check that the email account has valid OAuth tokens. Try removing and re-adding the account in the Iris UI.
