---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: agent-api-integration
slug: uc-005-agent-api-integration
---

# Use Case: Agent API Integration

## Summary

An external agent (bot, script, or AI assistant) uses a scoped API key to search emails, read message content, and optionally draft or send replies through the Iris Agent API. All actions are audit-logged.

## Actors

- **User**: Creates and manages API keys via the Iris web UI.
- **Agent**: An external program that authenticates via Bearer token and calls agent endpoints.
- **System**: The Iris backend, handling auth, scoping, and audit logging.

## Preconditions

- Iris server is running with at least one synced email account.
- The user has created an API key via the Settings page with appropriate permissions.
- The agent has the raw API key (shown once at creation time).

## Flow: Create API Key

1. User navigates to Settings > API Keys.
2. User clicks "Create Key" and fills in: name (e.g., "Support Bot"), permission level (e.g., "read_only"), and optionally scopes it to a specific account.
3. System generates a key: `iris_` + 32 random hex chars. Only the SHA-256 hash is stored.
4. The raw key is displayed once. User copies it and provides it to the agent.

## Flow: Agent Searches Emails

1. Agent sends `GET /api/agent/search?q=invoice+overdue` with header `Authorization: Bearer iris_{key}`.
2. Agent auth middleware extracts the Bearer token, hashes it with SHA-256, and looks up the hash in the api_keys table.
3. If the key is valid and not revoked, the ApiKey struct is injected into the request.
4. The search handler checks that the key has "search" permission (available to all permission levels).
5. If the key is scoped to an account, an account_id filter is added to the query.
6. FTS5 search executes and returns matching results.
7. The action is logged to the audit_log table: action="search", status="success", details="q=invoice overdue, results=5".

## Flow: Agent Reads a Message

1. Agent sends `GET /api/agent/messages/{id}` with the Bearer token.
2. Auth middleware validates the key.
3. Handler checks "read" permission.
4. If the key is scoped to an account, the handler verifies the message belongs to that account (returns 403 if not).
5. Full message detail is returned.
6. Audit logged: action="read", resource_type="message", resource_id="{id}".

## Flow: Agent Sends an Email

1. Agent sends `POST /api/agent/send` with Bearer token and JSON body:
   ```json
   {
     "account_id": "...",
     "to": ["recipient@example.com"],
     "subject": "Re: Invoice Follow-up",
     "body_text": "Following up on the overdue invoice...",
     "in_reply_to": "<original@example.com>"
   }
   ```
2. Auth middleware validates the key.
3. Handler checks "send" permission (requires send_with_approval or autonomous level).
4. Account scope is verified if the key is scoped.
5. The account's OAuth token is refreshed if needed.
6. Email is built via lettre and sent via SMTP.
7. A copy is stored in the local messages table with folder "Sent."
8. Audit logged: action="send", status="success", details="to=[...], subject=Re: Invoice Follow-up".

## Postconditions

- The agent has received search results, message content, or send confirmation.
- All actions are recorded in the audit_log table.
- The API key's `last_used_at` timestamp is updated.

## Error Scenarios

| Scenario | System Response |
|---|---|
| Invalid API key | 401 Unauthorized |
| Revoked API key | 401 Unauthorized |
| Insufficient permissions | 403 Forbidden; audit logged with status "forbidden" |
| Account scope mismatch | 403 Forbidden; audit logged with details "account scope mismatch" |
| OAuth token refresh failure | 502 Bad Gateway; audit logged with status "error" |
| SMTP send failure | 502 Bad Gateway; audit logged with status "error" |

## Related Features

- fh-010-agent-api
- fh-004-compose-send (SMTP pipeline reuse)
