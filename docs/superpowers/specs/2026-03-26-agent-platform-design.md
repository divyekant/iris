# Agent Platform & Memories v5 Integration

**Date:** 2026-03-26
**Status:** Draft
**Scope:** Unified agent auth, Memories v5 search/store upgrade, MCP alignment, reply/forward endpoints

---

## Problem

Iris has 200+ protected API routes but agents can only access 5 of them through a separate `/api/agent/*` namespace. The remaining capabilities (labels, contacts, analytics, AI features, subscriptions, webhooks, etc.) are locked behind session auth. This forces agent developers to work with a fraction of the platform.

Separately, Iris's Memories integration uses a v1-era API surface (6 of 84 endpoints). Memories v5 adds graph-aware search, temporal filtering, and document dating that would make semantic search significantly more useful for agents querying email.

## Core Principle

**If the UI can do it, agents can do it.** One API, two auth methods.

---

## Section 1: Unified Authentication Middleware

### Current Architecture

```
/api/agent/*  -> agent_auth_middleware  (5 endpoints, API key Bearer token)
/api/*        -> session_auth_middleware (200+ endpoints, session token/cookie)
```

### Target Architecture

```
/api/*        -> unified_auth_middleware (200+ endpoints, session OR API key)
```

### Middleware Logic

The unified middleware replaces `session_auth_middleware` on protected routes:

1. Check `Authorization: Bearer iris_...` header -> API key auth
   - Hash the key with SHA-256
   - Look up in `api_keys` table
   - Verify not revoked
   - Extract permission level and optional account scope
   - Attach `AgentAuth { key_id, permission, account_id }` to request extensions
   - Log to audit trail
   - No CSRF checks needed (Bearer tokens are not auto-attached by browsers)

2. If no Bearer token, check `X-Session-Token` header or `iris_session` cookie -> session auth
   - Validate against startup session token
   - CSRF checks (same-origin for cookie-based mutations)
   - Attach `SessionAuth` to request extensions (full access)
   - If both `iris_session` cookie AND `Authorization: Bearer` are present, Bearer takes precedence and CSRF is skipped (the caller is explicitly providing credentials, not relying on ambient cookies)

3. Neither present -> 401 Unauthorized

### Permission Model

API keys carry one of four permission levels (matching existing `api_keys.permission` column values). Each level includes all permissions below it:

| Level | Allows | Example Operations |
|-------|--------|-------------------|
| `read_only` | Most GET endpoints | List messages, get thread, search, analytics, contacts |
| `draft_only` | read_only + non-send mutations | Save draft, batch label/archive, create label, snooze, notes |
| `send_with_approval` | draft_only + sending | Send, reply, forward, cancel send |
| `autonomous` | send_with_approval + config/admin | Change settings, manage webhooks, delegation playbooks |

**Sensitive GET endpoints** require elevated permissions:
- `GET /api/config`, `GET /api/config/ai` -> `autonomous` (exposes server configuration)
- `GET /api/api-keys` -> `autonomous` (enumerates API keys)
- `GET /api/audit-log` -> `autonomous` (audit trail)
- `GET /api/webhooks` -> `draft_only` (webhook URLs are operational data)

### Account Scope Enforcement

API keys have an optional `account_id` field. When set, the middleware attaches the account constraint to request extensions. Enforcement:

- Handlers that accept an `account_id` query parameter: the middleware overrides it with the scoped account. The agent cannot query other accounts.
- Handlers that return cross-account data (e.g., `GET /api/messages` without account filter): the middleware injects the account filter automatically.
- Handlers that operate on a specific message/thread by ID: the handler verifies the resource belongs to the scoped account, returning 403 if not.
- Unscoped API keys (account_id = None) have access to all accounts.

### Permission Enforcement

Handlers that perform restricted operations check the permission level from request extensions:

```rust
fn require_permission(extensions: &Extensions, needed: Permission) -> Result<(), StatusCode> {
    // SessionAuth always passes (full access)
    // AgentAuth checks permission hierarchy
}
```

### Rate Limiting for API Keys

The rate limit key extractor is updated to use the API key ID when Bearer auth is present, instead of falling back to `"__anonymous__"`. Each API key gets its own rate limit bucket (same 500 burst / 5 req/sec as session auth). This prevents one misbehaving agent from exhausting another agent's rate limit.

### Backwards Compatibility

The existing 5 `/api/agent/*` endpoints remain as aliases. They forward to the same handler functions used by the unified routes. No breaking changes for existing integrations.

### Audit Trail

All agent API key operations are logged to the existing `audit_log` table:
- `key_id`, `action`, `resource_type`, `resource_id`, `details`, `status`
- Already implemented for the 5 agent endpoints; now covers all routes

---

## Section 2: Memories v5 Search & Store Upgrade

### Store Changes (memories_store job)

The `memories_store` background job indexes email content into Memories after IMAP sync.

**Current upsert call:**
```
upsert(text, source="iris/{account_id}/messages/{msg_id}", key=rfc_message_id)
```

**Updated upsert call:**
```
upsert(text, source, key, document_at=email_sent_date_iso8601)
```

The `document_at` field tells Memories when the email was sent, not when it was indexed. This enables accurate temporal search (e.g., "emails from last week").

### MemoriesClient API Changes (ai/memories.rs)

Additive changes to the HTTP client — existing callers unaffected:

**upsert / upsert_batch:**
- Add optional `document_at: Option<String>` field to the request body
- Pass the email's sent date as ISO 8601

**search:**
- Add optional parameters: `since`, `until`, `graph_weight`
- Default `graph_weight: 0.1` for graph-expanded results
- Default `recency_weight: 0.0` — email relevance is not recency-based
- Default `confidence_weight: 0.0` — skip confidence decay for email content

### Search Integration (search.rs)

The existing semantic search path (`?semantic=true`) gets upgraded:

- Parse `since` and `until` query parameters, pass to Memories search
- Pass `graph_weight`, `recency_weight`, `confidence_weight` with email-tuned defaults
- These parameters flow through the same search endpoint agents now access via unified auth

### What We're Not Doing

- **Not switching to the extraction pipeline.** Iris uses direct upsert, not Memories extraction (AUDN). The upsert path is simpler and we control the text format. Auto-linking (which requires extraction) is deferred.
- **Not creating explicit memory links.** Thread relationships and contact links could be built via the Memories links API, but this is a follow-up scope.
- **Not wiring feedback signals.** Iris has AI feedback (thumbs up/down). Piping these to Memories `POST /search/feedback` for ranking improvements is a natural follow-up.
- **Not backfilling existing emails.** Emails indexed before this upgrade will lack `document_at`. Temporal searches with `since`/`until` may miss these older entries. A backfill job (re-upsert with the email's date) is a follow-up if needed — most users will have recent email as the primary agent use case.

---

## Section 3: MCP Auth Alignment

### Current State

MCP sessions are created via `POST /api/mcp/initialize`, which requires session auth. Agents using MCP must first obtain a session token through bootstrap — awkward for programmatic access.

### Change

`POST /api/mcp/initialize` accepts API key auth (via unified middleware). The MCP session inherits the API key's permission level. Tool calls within the session are permission-checked against that level.

### Tool Coverage

The 26 existing MCP tools remain. They already cover the core agent use case (search, read, send, draft, summarize, etc.). After unified auth:
- REST API is the comprehensive surface (200+ endpoints)
- MCP is the curated tool set for LLM-based agents
- Both authenticate the same way

### MCP Tool Permission Mapping

MCP tool calls are permission-checked against the session's auth level. The mapping rule:
- **read_only:** search_emails, read_email, list_inbox, list_threads, get_thread, get_thread_summary, get_contact_profile, get_inbox_stats, extract_tasks, extract_deadlines
- **draft_only:** create_draft, manage_draft, archive_email, star_email, bulk_action
- **send_with_approval:** send_email, chat (when chat proposes sending)
- **autonomous:** any tool (no restrictions)

Tool calls that exceed the session's permission level return an error in the MCP response, not an HTTP 403.

### What We're Not Doing

- **Not rewriting MCP tools to call REST handlers.** The tools currently call database functions directly. Refactoring them to go through REST handlers is a follow-up that ensures perfect parity, but isn't required for this scope.
- **Not building a standalone MCP server binary.** MCP runs inside Iris's HTTP server.

---

## Section 4: Reply & Forward Endpoints

### Problem

The UI composes replies by building the full email client-side (threading headers, quoted body, recipient resolution) and sending via `POST /api/send`. Agents shouldn't need to construct In-Reply-To, References, or Re:/Fwd: subject prefixes manually.

### New Endpoints

**`POST /api/reply`** (requires `send` permission)

Request:
```json
{
  "message_id": "abc123",
  "body": "Thanks, I'll review the contract.",
  "reply_all": false
}
```

Server handles:
- In-Reply-To header from original message's Message-ID
- References chain from original thread
- Recipient resolution (reply: sender only; reply_all: sender + all To/CC minus self)
- Re: subject prefix
- Quoted body inclusion

**`POST /api/forward`** (requires `send` permission)

Request:
```json
{
  "message_id": "abc123",
  "to": ["colleague@example.com"],
  "body": "FYI - see below."
}
```

Server handles:
- Fwd: subject prefix
- Original message body inclusion
- Attachment forwarding

**`POST /api/drafts/reply`** (requires `draft` permission)

Same parameters as reply, creates a draft for human review instead of sending.

**`POST /api/drafts/forward`** (requires `draft` permission)

Same parameters as forward, creates a draft instead of sending.

### Account Resolution

The `account_id` is derived from the original message's record in the database — not provided by the caller. The server looks up `message_id`, reads its `account_id`, and uses that account's SMTP credentials. If the account is inactive or missing OAuth tokens, the endpoint returns 400 with an error describing the issue.

For API keys with account scope: the server verifies the original message belongs to the scoped account, returning 403 if not.

### Error Responses

- `404` — message_id not found
- `400` — account inactive or SMTP credentials unavailable
- `403` — message belongs to an account outside the API key's scope
- `422` — missing required fields (body)

### Implementation

These endpoints call existing compose/send logic internally. The `send_message` handler in `compose.rs` already handles email construction. The `redirect_message` handler already resolves threading context from a message ID — this pattern is extracted into a shared utility for reply/forward. The reply/forward endpoints resolve headers and quoted content, then delegate to the existing send or draft-save path.

---

## Implementation Strategy

### Phase 1: Unified Auth (foundation)
- Create `unified_auth_middleware` combining session and API key auth
- Add permission-checking utility for handlers
- Replace `session_auth_middleware` on protected routes
- Keep `/api/agent/*` as aliases
- Update tests

### Phase 2: Memories v5 Upgrade (backend)
- Add `document_at`, `since`, `until`, `graph_weight` to MemoriesClient
- Update `memories_store` job to pass email sent date
- Update search to pass v5 parameters
- Tune weights for email domain (zero recency/confidence decay)

### Phase 3: Reply & Forward Endpoints
- `POST /api/reply` and `POST /api/forward`
- `POST /api/drafts/reply` and `POST /api/drafts/forward`
- Threading header construction from message_id
- Quoted body formatting

### Phase 4: MCP Auth Alignment
- Update MCP initialize to accept API key auth
- Permission-check MCP tool calls against key permission level

---

## Testing Strategy

- **Unit tests:** Permission checking logic, unified middleware auth paths
- **Integration tests:** Agent API key authenticates against protected routes, permission denial for insufficient level, backwards compat for `/api/agent/*`
- **Security tests:** API key can't access config endpoints with `read` permission, CSRF still enforced for session cookie auth, revoked keys rejected
- **Memories tests:** `document_at` passed correctly, temporal search returns date-bounded results, graph_weight affects result ranking
- **Reply/forward tests:** Threading headers correct, reply-all recipients resolved, Fwd: prefix applied, draft variants save without sending

---

## What This Unlocks

After this work, an external agent can:

1. Create an API key in Iris Settings UI with appropriate permissions
2. Authenticate via `Authorization: Bearer iris_...`
3. Access any of 200+ endpoints — search, read, organize, compose, send, analyze
4. Use semantic search with temporal bounds ("emails from Sarah last week about deployment")
5. Draft replies for human review, or send directly with `send` permission
6. Connect via MCP for LLM-native tool use

Iris becomes a full local email API that any agent — Claude Code, custom scripts, autonomous workflows — can plug into.
