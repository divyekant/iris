---
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
release: v0.4.0
---

# Release Notes: Iris v0.4.0

**Release Date**: 2026-03-26

This release transforms Iris from a capable email client into an agent-ready platform. The headline change is unified authentication that lets external AI agents and automation tools use the same API surface as the web UI, with granular permission controls. Six new AI-powered showcase features demonstrate the intelligence layer, and production hardening makes the system ready for real deployment.

---

## Agent Platform

### Unified Auth Middleware
- All 200+ protected routes now accept both session tokens (browser UI) and API key Bearer tokens (agents).
- Single middleware (`unified_auth_middleware`) replaces the dual-auth approach. Check order: Bearer token, then session token, then 401.
- `AuthContext` enum tags every request as `Session` (full access) or `Agent` (permission-scoped).

### Permission Model
- Four-tier permission hierarchy: `read_only`, `draft_only`, `send_with_approval`, `autonomous`.
- Higher permissions satisfy lower ones (e.g., `autonomous` can do everything `read_only` can).
- Session auth bypasses all permission checks -- the web UI retains full access.

### API Key Management
- Keys use `iris_` prefix + 32 random hex chars. SHA-256 hashed before storage.
- Create (`POST /api/api-keys`), list (`GET /api/api-keys`), revoke (`DELETE /api/api-keys/{id}`).
- Optional account scope restricts a key to a single email account.
- `last_used_at` timestamp updated on each use.

### Reply/Forward Endpoints
- `POST /api/reply` and `POST /api/forward` handle server-side threading with proper `In-Reply-To` and `References` headers.
- `POST /api/drafts/reply` and `POST /api/drafts/forward` create draft versions without sending.
- Reply-all correctly deduplicates recipients and excludes self from To/CC.
- Account scope enforced -- agents cannot reply to messages outside their scope.

### Per-Key Rate Limiting
- Agent requests get separate rate limit buckets keyed by `agent:{key_prefix}`.
- Each API key is independently rate-limited, separate from the UI session.

### Audit Logging
- All agent actions logged to `audit_log` with key_id, action, resource type/id, status, and timestamp.
- Queryable via `GET /api/audit-log` with optional key filter and pagination.

---

## Memories v5 Integration

### Temporal Search
- `document_at` field on email upserts anchors entries to the email's original send date.
- `since` and `until` query parameters enable date-range filtering in semantic search.
- Search API (`GET /api/search?semantic=true`) passes temporal params through to Memories.

### Graph-Aware Search
- `graph_weight` parameter (set to 0.1 for email) provides a small boost for entity-related results.
- Entity relationships from the knowledge graph influence search ranking.

### Email-Tuned Decay
- `recency_weight` and `confidence_weight` set to 0.0 for email search.
- Old emails rank equally with new ones when content matches -- email relevance is topic-based, not time-based.
- Explicit `since/until` parameters provide temporal filtering when needed.

### Config Hot-Reload
- `MemoriesClient::update_config()` allows changing the Memories URL and API key at runtime.
- Docker URL rewriting: `localhost` automatically becomes `host.docker.internal` when `BIND_ALL` is set.

---

## MCP Permission Checking

### Tool-to-Permission Mapping
- Every MCP tool call checks the caller's permission level before execution.
- 10 read-only tools, 5 draft-level tools, 2 send-level tools.
- Unknown tools default to `autonomous` (rejected by tool existence check).

### API Key MCP Sessions
- API keys can create MCP sessions via `POST /api/mcp/initialize`.
- The `api_key_id` is derived from the auth context, not the request body.
- Permission checks apply on each tool call, not at session creation.

### MCP Error Format
- Permission denials return MCP-formatted errors (HTTP 200 with `status: "permission_denied"`) rather than HTTP 403.
- Aligns with MCP protocol expectations where transport succeeds but tools report errors.

---

## Showcase Features

### Delegation Agent
- Playbook-based email automation with trigger conditions (sender domain, subject, category, intent).
- Five action types: `auto_reply`, `draft_reply`, `forward`, `archive`, `label`.
- Configurable confidence threshold (default 0.85).
- Action history with undo capability and daily summary dashboard.
- CRUD API for playbooks plus process/actions/summary endpoints.

### Custom Categories
- User-defined email categories beyond built-in Primary/Updates/Social/Promotions.
- AI analysis of 200 recent messages suggests new categories.
- Accept/dismiss workflow for AI-suggested categories.
- Explain endpoint provides AI reasoning for category assignment.

### Writing Style Learning
- Analyzes up to 200 sent emails to extract 6 style traits: greeting, signoff, tone, avg_length, formality, vocabulary.
- Each trait includes confidence score and example excerpts.
- Style traits feed into auto-draft generation.

### Auto-Draft
- AI-generated draft replies for routine incoming emails.
- Incorporates user's writing style traits for natural-sounding drafts.
- Configurable sensitivity: conservative, balanced, aggressive.
- Idempotent generation (second call returns existing draft).
- Feedback loop: accept, edit, or reject.

### Knowledge Graph
- Entity extraction: person, org, project, date, amount.
- Relationship mapping: works_at, manages, collaborates_with, part_of, reports_to.
- Timeline event extraction with date precision (day/week/month/quarter/year).
- Entity resolution via canonical name + alias lookup.
- Graph query API and entity listing.

### Temporal Reasoning
- Natural-language temporal queries ("emails around the product launch").
- AI resolves queries to concrete date ranges using known timeline events.
- Falls back to last 30 days when no temporal reference is found.
- Timeline event listing from knowledge graph extraction.

---

## Production Hardening

### Security Headers
- Four headers on every response: `x-content-type-options: nosniff`, `x-frame-options: DENY`, `referrer-policy: strict-origin-when-cross-origin`, `permissions-policy: camera=(), microphone=(), geolocation=()`.

### Non-Root Docker
- Multi-stage build: Rust builder, Node frontend, Debian slim runtime.
- Runtime runs as non-root user `iris` (UID 1001).
- Docker HEALTHCHECK with 30s interval, 5s timeout, 10s start period.

### CI/CD Pipeline
- Four parallel jobs: Backend (fmt + clippy + test), Frontend (check + build), Docker Build, Security Audit.
- `cargo clippy -- -D warnings` treats all warnings as errors.
- `cargo audit` and `npm audit --audit-level=moderate` for dependency security.
- Cargo and npm caches for faster builds.

### Request Size Limit
- 25 MB body limit on all routes via `DefaultBodyLimit`.

### Health Check
- `GET /api/health` checks DB (5s timeout), AI providers, and Memories.
- Version hardcoded to `0.3.0` (env! macro unreliable in Docker cache).
- Status is "ok" or "degraded" (DB-only -- AI and Memories are optional).

### Auth Rate Limiting
- Auth endpoints: 10 burst, 1/sec sustained (brute-force protection).
- General API: 500 burst, ~5/sec sustained.
- Separate buckets for agents (keyed by API key prefix) and sessions.

### Provider Hot-Reload
- `ProviderPool::reload()` replaces all AI providers at runtime.
- Round-robin with fallback: tries each provider in rotation.
- Supported: Ollama, Anthropic, OpenAI.

### CORS Warning
- `IRIS_CORS_ORIGINS` env var configures allowed origins.
- Falls back to localhost dev defaults with a `tracing::warn!` log message.

---

## Breaking Changes

None. All changes are additive. The legacy `/api/agent/*` routes remain functional alongside the new unified auth.

---

## Migration Notes

- **API key users**: Existing API keys continue to work on legacy agent routes. To use the unified auth on standard routes, include `Authorization: Bearer iris_...` on any endpoint.
- **Memories users**: If upgrading to Memories v5 server, email entries will not have `document_at` until re-synced. Temporal filtering (`since/until`) only works on entries with this field.
- **Docker users**: The container now runs as non-root. Volume mounts for `/app/data` must be writable by UID 1001.
- **CI users**: The pipeline expects `cargo audit` to be available. The workflow installs it if missing.
