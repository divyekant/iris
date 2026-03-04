---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Error Reference

This page documents the HTTP error responses you may encounter when using Iris, along with common causes and how to resolve them.

## 400 Bad Request

**Meaning:** Your request is malformed or missing required information.

**Common Causes:**

| Scenario | Error Message | Resolution |
|---|---|---|
| Invalid batch action | -- | Use one of the valid actions: `archive`, `delete`, `mark_read`, `mark_unread`, `star`, `unstar` |
| Empty or oversized batch | -- | Provide between 1 and 1,000 message IDs in a batch update |
| Invalid AI assist action | -- | Use one of: `rewrite`, `formal`, `casual`, `shorter`, `longer` |
| Invalid API key permission | `"invalid permission: {value}"` | Use one of: `read_only`, `draft_only`, `send_with_approval`, `autonomous` |
| Invalid feedback field | -- | Use one of: `category`, `priority_label`, `intent` |
| Invalid feedback value | -- | Use a valid value for the field (see [AI Feedback](features/feat-008-ai-feedback.md) for valid values) |
| Malformed email address in send | `"error building email: ..."` | Check that all email addresses in `to`, `cc`, and `bcc` are valid |
| Missing required fields | `"error": "..."` | Include all required fields in your request body |

**Example Response:**

```json
{
  "error": "invalid permission: superadmin"
}
```

## 401 Unauthorized

**Meaning:** Your request is not authenticated.

**Common Causes:**

| Scenario | Resolution |
|---|---|
| Missing `X-Session-Token` header on a protected endpoint | Retrieve the session token via `GET /api/auth/bootstrap` from the browser, then include it as `X-Session-Token` in subsequent requests |
| Invalid or expired session token | The session token changes on every server restart. Retrieve a fresh token via the bootstrap endpoint. |
| Missing `Authorization` header on an agent endpoint | Include `Authorization: Bearer iris_your_key` in agent API requests |
| Invalid or revoked API key | Create a new API key in **Settings > API Keys** |

**Note:** Iris generates a new random session token each time the server starts. If you restart the server, you need a new session token.

## 403 Forbidden

**Meaning:** You are authenticated, but not authorized for this action.

**Common Causes:**

| Scenario | Error Message | Resolution |
|---|---|---|
| Bootstrap endpoint called from a cross-origin request | -- | The `/api/auth/bootstrap` endpoint only responds to same-origin or same-site browser requests (determined by the `Sec-Fetch-Site` header). Access it from the Iris frontend in a browser, not from an external tool. |
| Agent API key lacks required permission | `"insufficient permissions"` | Create a new API key with a higher permission level |
| Agent API key is scoped to a different account | `"message not in scope"` or `"account not in scope"` | Use an unscoped key, or use a key scoped to the correct account |

**Example Response:**

```json
{
  "error": "insufficient permissions"
}
```

## 404 Not Found

**Meaning:** The requested resource does not exist.

**Common Causes:**

| Scenario | Resolution |
|---|---|
| Message ID does not exist | Verify the message ID. Use the search endpoint to find valid message IDs. |
| Thread ID does not exist or has no messages | Verify the thread ID. Thread IDs are derived from email Message-ID headers. |
| Account ID does not exist | List accounts via `GET /api/accounts` to find valid account IDs. |
| Draft ID does not exist or was already deleted | List drafts to find valid draft IDs. |
| API key ID does not exist (when revoking) | List API keys to find valid key IDs. |
| Chat message not found (when confirming an action) | Ensure the `message_id` and `session_id` are correct. |

**Example Response:**

```json
{
  "error": "message not found"
}
```

## 413 Payload Too Large

**Meaning:** Your request body exceeds the maximum allowed size.

**Common Causes:**

| Scenario | Limit | Resolution |
|---|---|---|
| Chat message too long | 50,000 characters | Shorten your chat message |
| AI assist content too long | 50,000 characters | Shorten the text you are asking the AI to rewrite |

## 500 Internal Server Error

**Meaning:** Something went wrong on the server side.

**Common Causes:**

| Scenario | Resolution |
|---|---|
| Database connection failure | Check that the SQLite database file path (`DATABASE_URL`) is accessible and not corrupted. Check disk space. |
| Database migration failure | Check server logs for migration errors. The database may need to be reset if a migration fails partway. |
| Search query error | Check that your search query does not contain malformed FTS5 syntax. Try a simpler query. |

**What to do:** Check the server logs (stdout) for detailed error messages. The log includes the specific error that caused the 500 response.

## 502 Bad Gateway

**Meaning:** Iris could not communicate with an external service.

**Common Causes:**

| Scenario | Error Message | Resolution |
|---|---|---|
| Ollama unreachable | -- | Check that Ollama is running at the configured `OLLAMA_URL`. Run `curl http://localhost:11434/api/tags` to verify. |
| SMTP send failure | `"Connection refused"` or similar | Check that the email account's SMTP server is reachable. For OAuth accounts, the token may have expired -- try reconnecting the account. |
| OAuth token refresh failure | `"token refresh: ..."` | The OAuth refresh token may have expired or been revoked. Remove and re-add the account in **Settings > Accounts**. |

## 503 Service Unavailable

**Meaning:** The requested AI feature is not configured.

**Common Causes:**

| Scenario | Resolution |
|---|---|
| AI chat called but AI is disabled | Enable AI in **Settings > AI** and select a model |
| Thread summarization called but no model selected | Select an AI model in **Settings > AI** |
| AI assist called but AI is disabled | Enable AI and select a model |

**Resolution:** Go to **Settings > AI**, verify the Ollama connection, select a model, and enable AI processing.

## Checking System Health

To quickly diagnose issues, check the health endpoint:

```bash
curl http://localhost:3000/api/health
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

| Field | Healthy | Unhealthy | Impact |
|---|---|---|---|
| `db` | `true` | `false` | All features broken -- database is required |
| `ollama` | `true` | `false` | AI features unavailable (classification, chat, summarization, writing assist) |
| `memories` | `true` | `false` | Semantic search unavailable, falls back to keyword search |
