---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
troubleshooting: agent-api
slug: ts-004-agent-api
---

# Troubleshooting: Agent API

## Overview

This guide covers issues with API key authentication, permission errors, account scoping, SMTP sending from agents, and audit log review.

## Diagnostic Checklist

1. **Key validity**: Ensure the API key is the full `iris_{32 hex chars}` string (37 characters total). The key must not be revoked.
2. **Auth header format**: Must be `Authorization: Bearer iris_{key}`. Note the space after "Bearer."
3. **Permission level**: Check what permission level the key was created with. Use `GET /api/api-keys` (session-authenticated) to list active keys.
4. **Audit log**: Check `GET /api/audit-log` for recent entries from the agent's key.

## Issue: 401 Unauthorized on all agent requests

**Symptoms**: Every agent endpoint returns 401.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Missing Authorization header | Include `Authorization: Bearer iris_{key}` in the request. |
| Malformed header | Must be exactly `Bearer ` (with space) followed by the key. Case-insensitive "bearer" is accepted. |
| Wrong key | Verify the key is the one displayed at creation time. Keys cannot be recovered after creation. |
| Key revoked | Check the api_keys list in Settings. If revoked, create a new key. |
| Key hash mismatch | If the key was modified (extra spaces, newlines, truncation), the SHA-256 hash will not match. |

## Issue: 403 Forbidden on specific operations

**Symptoms**: Some operations work (search, read) but others return 403.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Insufficient permission level | `read_only` keys cannot draft or send. `draft_only` cannot send. Check the key's permission level and upgrade if needed. |
| Account scope mismatch | If the key is scoped to account_id "A" and the agent tries to read a message from account "B," the request is rejected. Verify the message/thread belongs to the scoped account. |
| Wrong operation for permission | `read_only` allows: read, search. `draft_only` adds: draft. `send_with_approval` adds: send. Check the permission hierarchy. |

**How to diagnose**: Check the audit log. Forbidden requests are logged with status "forbidden" and often include details like "account scope mismatch."

## Issue: Agent send returns 502 Bad Gateway

**Symptoms**: The send request is authenticated and has permission, but returns 502.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| OAuth token refresh failed | The account's OAuth tokens may be expired or revoked. Re-authenticate the account via the web UI. |
| SMTP connection failure | The SMTP server may be unreachable. Check network connectivity to the SMTP host. |
| Invalid recipient address | The `to` field must contain valid email addresses. |
| SMTP authentication failure | XOAUTH2 may be rejected if the token is invalid. Check the account's OAuth status. |

**Audit log clue**: The audit entry will have status "error" with details like "token refresh failed: ..." or "smtp failed: ...".

## Issue: Agent search returns empty results

**Symptoms**: Search with a known query returns empty results.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Empty query | The `q` parameter must be non-empty. |
| Account scope too narrow | If the key is scoped to an account that has no matching messages, results will be empty. |
| FTS5 query syntax issue | Special characters may cause FTS5 errors. The agent search wraps terms in quotes, but unusual characters may still cause issues. |
| No messages synced | Verify that the account has synced messages. |

## Issue: Agent draft creation fails

**Symptoms**: `POST /api/agent/drafts` returns an error.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Permission too low | Requires `draft_only`, `send_with_approval`, or `autonomous`. |
| Account scope mismatch | If scoped, the draft's `account_id` must match the key's account_id. |
| Invalid request body | Ensure `account_id` and `body_text` are provided. |

## Reviewing the Audit Log

The audit log is the primary tool for understanding agent behavior:

**Access**: `GET /api/audit-log?limit=50&offset=0` (session-authenticated, not agent-authenticated).

**Filtering**: Add `api_key_id={id}` to filter entries for a specific key.

**Fields**:
- `api_key_id` -- which key made the request
- `key_name` -- human-readable key name (joined from api_keys table)
- `action` -- what was attempted (search, read, draft, send)
- `resource_type` -- type of resource (message, thread)
- `resource_id` -- specific ID accessed (if applicable)
- `details` -- context (query text, recipient, error message)
- `status` -- outcome (success, forbidden, error)
- `created_at` -- timestamp

**Common patterns to look for**:

| Pattern | Meaning |
|---|---|
| Multiple "forbidden" entries | Agent is trying operations beyond its permission level |
| "account scope mismatch" in details | Agent is trying to access a different account than scoped |
| "token refresh failed" errors | The underlying email account needs re-authentication |
| High volume of "search" entries | Agent is actively querying; verify this is expected behavior |
| "send" entries with "success" | Agent successfully sent emails; verify recipients are correct |

## Key Rotation Procedure

1. Create a new key with the same name and permissions via Settings > API Keys.
2. Copy the new raw key.
3. Update the agent configuration with the new key.
4. Verify the agent can authenticate with the new key.
5. Revoke the old key via Settings > API Keys > Revoke.
6. Confirm in the audit log that only the new key is being used.

## Security Considerations

- API keys are transmitted in the Authorization header. Use HTTPS for any non-localhost deployment.
- Revoked keys cannot be un-revoked. Create a new key if needed.
- The audit log retains all entries indefinitely. Review periodically for unexpected agent activity.
- Scoped keys are the recommended approach for agents that only need access to a single account.
