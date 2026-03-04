---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: agent-api
slug: fh-010-agent-api
---

# Feature Handoff: Agent API

## What It Does

The Agent API provides a set of REST endpoints that external agents (bots, scripts, AI assistants) can use to search, read, draft, and send emails. Access is controlled via scoped API keys with four permission levels. All agent actions are audit-logged.

## How It Works

### API Key Management

**Key Format**: `iris_` + 32 random hex characters (37 chars total). Only the SHA-256 hash is stored in the database. The raw key is shown to the user exactly once at creation time.

**Key Prefix**: The first 12 characters of the raw key are stored as `key_prefix` for identification in the UI and audit logs.

**CRUD via session-authenticated endpoints**:
- `POST /api/api-keys` -- create key (name, permission, optional account_id)
- `GET /api/api-keys` -- list active (non-revoked) keys
- `DELETE /api/api-keys/{id}` -- revoke key (soft delete: sets is_revoked=1 and revoked_at)

### Permission Hierarchy

Four permission levels, each inheriting actions from lower levels:

| Level | Actions Allowed |
|---|---|
| `read_only` | read, search |
| `draft_only` | read, search, draft |
| `send_with_approval` | read, search, draft, send |
| `autonomous` | read, search, draft, send, execute, configure |

Permission checking is done by `has_permission(key_permission, required_action)` which looks up the action in a static map of allowed actions per level.

### Account Scoping

API keys can optionally be scoped to a specific `account_id`. When scoped:
- Search results are filtered to that account.
- Read requests verify the message belongs to the scoped account (returns 403 if not).
- Draft and send requests must target the scoped account.

When unscoped (account_id is null), the agent can access all accounts.

### Agent Auth Middleware

Agent endpoints are protected by `agent_auth_middleware`:
1. Extracts the Bearer token from the `Authorization` header.
2. Hashes the token with SHA-256.
3. Looks up the hash in `api_keys` where `is_revoked = 0`.
4. If found, updates `last_used_at` and injects the `ApiKey` struct into request extensions.
5. If not found, returns 401 Unauthorized.

### Agent Endpoints

All prefixed under `/api/agent/`:

| Endpoint | Permission | Description |
|---|---|---|
| `GET /search?q=...` | search | FTS5 search with optional account scoping |
| `GET /messages/{id}` | read | Full message detail |
| `GET /threads/{id}` | read | All messages in a thread |
| `POST /drafts` | draft | Create a new draft |
| `POST /send` | send | Build and send an email via SMTP |

The send endpoint reuses the same SMTP pipeline as the user-facing compose flow: token refresh, email building via lettre, SMTP sending, and local storage of the sent message.

### Audit Logging

Every agent action is logged to the `audit_log` table:
- `api_key_id` -- which key was used
- `action` -- what action was attempted (search, read, draft, send)
- `resource_type` -- type of resource (message, thread)
- `resource_id` -- specific resource ID if applicable
- `details` -- additional context (query text, recipient, etc.)
- `status` -- success, forbidden, or error

The audit log is viewable via `GET /api/audit-log` (session-authenticated) with optional filtering by `api_key_id` and pagination.

## User-Facing Behavior

- In Settings, the API Keys section allows creating new keys with a name and permission level.
- The raw key is displayed once at creation time with a copy button.
- Active keys are listed with their prefix, permission, last used time, and a revoke button.
- The Audit Log section shows all agent actions with timestamps, key names, actions, and statuses.

## Configuration

No environment variables are needed. API keys are managed entirely through the web UI and stored in the SQLite database.

## Edge Cases and Limitations

- The raw API key is shown only once at creation. If the user loses it, the key must be revoked and a new one created.
- Key revocation is soft (sets a flag). The hash remains in the database.
- Audit log entries are never deleted. Over time, this table can grow large. There is no rotation or cleanup mechanism.
- The agent search endpoint uses FTS5 only (no semantic search toggle).
- The agent send endpoint sends email immediately. There is no approval workflow for `send_with_approval` level -- the name refers to the permission hierarchy level, not an actual approval step.
- API key validation requires a database lookup on every request. There is no caching layer.
- The `autonomous` permission level includes `execute` and `configure` actions, but no endpoints currently require these permissions.

## Common Questions

**Q: What is the difference between `send_with_approval` and `autonomous`?**
A: `send_with_approval` allows read, search, draft, and send. `autonomous` adds `execute` and `configure` actions, which are reserved for future features. Currently, both levels allow the same practical operations.

**Q: Can I restrict an API key to a specific account?**
A: Yes. When creating a key, specify an `account_id`. The agent can then only access messages and send from that account. Attempts to access other accounts return 403 Forbidden.

**Q: How do I rotate an API key?**
A: Create a new key with the same name and permissions, update the agent to use the new key, then revoke the old key.

**Q: Is the API key transmitted securely?**
A: API keys are sent in the `Authorization: Bearer {key}` header. In production, HTTPS should be used to encrypt the transport. The key is never stored in plaintext on the server -- only the SHA-256 hash is persisted.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| 401 Unauthorized | Invalid, revoked, or missing Bearer token | Verify the API key is correct and not revoked |
| 403 Forbidden on read | Key lacks read permission or account scope mismatch | Check key permission level and account_id scoping |
| 403 on send | Key permission is read_only or draft_only | Upgrade to send_with_approval or autonomous |
| Audit log shows "forbidden" entries | Agent attempted an unpermitted action | Review agent behavior; consider upgrading key permissions |
| Send returns 502 | OAuth token refresh failed or SMTP error | Check account OAuth credentials; verify SMTP connectivity |

## Related Links

- Source: `src/api/agent.rs`
- Database: `migrations/003_agent.sql` (api_keys, audit_log tables)
- SMTP: `src/smtp.rs`
- Frontend: Settings API Keys section, Audit Log section
