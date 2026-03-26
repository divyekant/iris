---
id: fh-027
type: feature-handoff
audience: internal
topic: agent-platform
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Feature Handoff: Agent Platform

## What It Does

The agent platform provides programmatic access to all Iris functionality through API keys with a tiered permission model. External agents (LLM assistants, automation scripts, MCP clients) authenticate with Bearer tokens and interact with the same routes the web UI uses, subject to permission checks. The system replaces the earlier agent-specific routes (`/api/agent/*`) with a unified auth layer that applies to all 200+ protected routes.

A unified auth middleware sits at the top of the route stack and accepts either a session token (browser UI) or a Bearer API key (agents). Every request gets tagged with an `AuthContext` that downstream handlers use for permission checks. API keys are SHA-256 hashed before storage, support per-key rate limiting, and carry an optional account scope that restricts access to a single email account.

## How It Works

### Unified Auth Middleware (`src/api/unified_auth.rs`)

The `unified_auth_middleware` function runs on all protected routes. It checks credentials in this order:

1. **Bearer token** -- If the `Authorization: Bearer iris_...` header is present, the key is SHA-256 hashed and looked up in the `api_keys` table. If found and not revoked, the request gets `AuthContext::Agent` with the key's permission level and optional account scope. The `last_used_at` timestamp is updated best-effort.
2. **Session token** -- If no Bearer token is found, the middleware looks for `X-Session-Token` header or `iris_session` cookie. A valid session token produces `AuthContext::Session` (full access). Cookie transport on mutating methods requires a same-origin browser context (CSRF check).
3. **No credentials** -- Returns 401.

### Permission Model

Four permission levels, ordered from least to most privileged:

| Level | DB Value | Allowed Actions |
|---|---|---|
| ReadOnly | `read_only` | read, search |
| DraftOnly | `draft_only` | read, search, draft |
| SendWithApproval | `send_with_approval` | read, search, draft, send |
| Autonomous | `autonomous` | read, search, draft, send, execute, configure |

Permission checks use `Permission::satisfies()` -- a higher permission always satisfies a lower one. `AuthContext::Session` always passes all permission checks.

### API Key Lifecycle (`src/api/agent.rs`)

- **Creation**: `POST /api/api-keys` (requires Autonomous permission or Session). Generates `iris_` + 32 random hex chars. The raw key is returned once and never stored. Only the SHA-256 hash goes into `api_keys`.
- **Listing**: `GET /api/api-keys` returns all non-revoked keys with prefix, permission, account scope, and last-used timestamp.
- **Revocation**: `DELETE /api/api-keys/{id}` sets `is_revoked = 1` and records `revoked_at`.
- **Audit logging**: All agent actions are logged to `audit_log` with key_id, action, resource type/id, status, and timestamp.

### Reply/Forward Endpoints (`src/api/reply_forward.rs`)

Server-side reply and forward endpoints handle email threading:

- `POST /api/reply` -- Requires `SendWithApproval`. Resolves the original message, builds quoted reply body, sets `In-Reply-To` and `References` headers, determines recipients (reply or reply-all).
- `POST /api/forward` -- Requires `SendWithApproval`. Builds forwarded message body with original metadata header block.
- `POST /api/drafts/reply` -- Requires `DraftOnly`. Creates a draft reply without sending.
- `POST /api/drafts/forward` -- Requires `DraftOnly`. Creates a draft forward without sending.

All endpoints enforce account scope for agent keys -- an agent scoped to `acct_123` cannot access messages from `acct_456`.

### Per-Key Rate Limiting (`src/api/rate_limit.rs`)

The `SessionTokenKeyExtractor` checks for Bearer tokens first. Agent requests get rate-limited under a bucket keyed by `agent:{key_prefix}` (first 16 chars). This means each API key gets its own rate limit bucket, separate from the UI session. The general rate limit is 500 burst / ~5 per second sustained.

## User-Facing Behavior

- Agents authenticate with `Authorization: Bearer iris_abc123...` on any API endpoint.
- Permission denied returns HTTP 403 with `{"error": "insufficient permission"}`.
- Revoked keys return HTTP 401.
- The raw key is displayed exactly once at creation. It cannot be recovered.
- Audit log shows all agent activity, viewable at Settings > Audit Log.
- Agent keys appear in Settings > API Keys with their prefix, permission level, and last-used timestamp.

## Configuration

| Setting | Location | Default | Description |
|---|---|---|---|
| Permission level | `api_keys.permission` | (required at creation) | One of: `read_only`, `draft_only`, `send_with_approval`, `autonomous` |
| Account scope | `api_keys.account_id` | `NULL` (all accounts) | Restricts key to a single email account |
| Rate limit (general) | `rate_limit.rs` | 500 burst, ~5/sec sustained | Per-session/key GCRA token bucket |
| Rate limit (auth) | `rate_limit.rs` | 10 burst, 1/sec sustained | Auth endpoints only |

## Edge Cases & Limitations

- **Key recovery is impossible.** The raw key is shown once. If lost, revoke and create a new one.
- **Session auth bypasses all permission checks.** There is no way to restrict the web UI session.
- **Account scope is optional.** An unscoped key can access all accounts. This is by design for personal use but would need tightening in multi-tenant scenarios.
- **Audit log does not capture session (UI) actions.** Only agent key actions are logged.
- **Reply/forward threading depends on stored raw_headers.** If `raw_headers` was not captured during sync, the References header chain may be incomplete.
- **Rate limit buckets are in-memory.** A server restart resets all rate limit counters.

## Common Questions

**Q: Can an agent create other API keys?**
A: Yes, but only if the agent's key has `autonomous` permission. Lower permission levels cannot create or revoke keys.

**Q: Does the unified auth middleware replace the old agent auth?**
A: The legacy `agent_auth_middleware` still exists on `/api/agent/*` routes for backward compatibility. New integrations should use Bearer auth on the standard API routes instead.

**Q: How does account scope interact with reply/forward?**
A: The `resolve_original` function checks `auth.account_scope()`. If the original message belongs to a different account than the key's scope, the request is rejected with 403.

**Q: What happens if an agent hits the rate limit?**
A: HTTP 429 is returned with `retry-after` and `x-ratelimit-after` headers. Successful responses include `x-ratelimit-limit` and `x-ratelimit-remaining` headers.

**Q: Are API keys tied to a user?**
A: No. Iris is single-user. Keys are just named credentials with permission levels. There is no user identity concept beyond session vs. agent.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| 401 on all agent requests | Key revoked, typo in key, or key not in DB | Verify key prefix matches a non-revoked entry in `api_keys` |
| 403 on specific endpoint | Key permission too low for the action | Check which permission level the endpoint requires |
| 429 Too Many Requests | Rate limit exceeded | Back off and retry after the `retry-after` header value |
| Reply creates malformed thread | Missing `raw_headers` on original message | Re-sync the message to capture raw headers |
| Audit log empty despite agent activity | Agent using legacy `/api/agent/*` routes | Switch to unified auth on standard routes |

## Related

- [fh-010-agent-api.md](fh-010-agent-api.md) -- Original agent API (legacy routes)
- [fh-012-auth-security.md](fh-012-auth-security.md) -- Session auth and security
- [fh-029-mcp-permissions.md](fh-029-mcp-permissions.md) -- MCP tool-to-permission mapping
- [fh-015-agent-infrastructure.md](fh-015-agent-infrastructure.md) -- Agent infrastructure foundations
