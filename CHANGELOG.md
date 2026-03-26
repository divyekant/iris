# Changelog

## 0.4.0 — 2026-03-26

### Agent Platform
- Unified authentication: API keys now access all 200+ routes (not just 5)
- Permission model: read_only, draft_only, send_with_approval, autonomous
- Per-API-key rate limiting (own bucket per key, 500 burst / 5 req/sec)
- Permission checks on config, send, draft, webhook, and admin endpoints
- Reply/forward endpoints: POST /api/reply, /api/forward, /api/drafts/reply, /api/drafts/forward
- Server-side threading header resolution (In-Reply-To, References, Re:/Fwd: prefix)
- MCP tool permission checks (tools gated by session permission level)

### Memories v5 Integration
- document_at field on email upserts (uses sent date, not index time)
- Temporal search: since/until date filters
- Graph-aware search: graph_weight parameter for expanded results
- Email-tuned defaults: zero recency/confidence decay
- SearchOptions struct for callers

### Production Hardening
- Non-root Docker container (user iris, UID 1001)
- Security headers: X-Content-Type-Options, X-Frame-Options, Referrer-Policy, Permissions-Policy
- Request body size limit (25 MB global)
- Health check timeout (5s on DB query via spawn_blocking)
- Auth rate limiting: 10 burst, 1/sec on login/bootstrap/oauth
- CORS warning on startup when IRIS_CORS_ORIGINS not set
- Fix dangerous unwrap() calls in agent.rs, smtp.rs, queue.rs, delegation.rs
- Pin Ollama image to 0.18.3

### Infrastructure
- CI/CD pipeline: cargo fmt/clippy/test, npm check/build, Docker build, cargo audit + npm audit
- Provider hot-reload on config save (no restart needed for new API keys)

## 0.3.0 — 2026-03-16

### Wave 3
- Delegation agent with playbooks
- Custom categories with AI analysis
- Writing style learning
- Auto-draft system
- Knowledge graph with entity extraction
- Temporal reasoning and timeline events

### Agent Infrastructure (Wave 3 Layer 2)
- Rust CLI binary (iris command) with 12 subcommands
- MCP server with 18 tools
- 27 MCP tests

### Post-Deploy Fixes
- Rate limit bumped from 100/min to 500/min
- Duplicate reply buttons removed
- Email font-src CSP relaxed for web fonts
- Trusted sender images feature
- Memories URL fix (localhost vs host.docker.internal)

## 0.2.0 — 2026-03-10

### Waves 1-2
- 10 feature batches: webhooks, structured data extraction, health reports,
  newsletter feeds, subscription management, analytics, attachment search,
  thread clustering, phishing detection, contact profiles
- VIP scoring and relationship intelligence
- Follow-up tracking and deadline extraction
- Newsletter digest and template suggestions
- Notification routing and effectiveness scoring
- Link safety scanning and social engineering detection
- Privacy reports and tracker detection

## 0.1.0 — 2026-03-04

Initial release. All 10 vertical slices complete.

### Email Client (V1-V4)
- OAuth2 authentication for Gmail and Outlook
- IMAP sync with IDLE push for real-time mail
- MIME parsing: HTML bodies, plain text, attachments, thread ID extraction
- Compose, reply, reply-all, forward with SMTP/XOAUTH2
- Auto-save drafts
- Batch actions: archive, delete, read/unread, star, category assignment
- Category tabs (Primary, Updates, Social, Promotions)
- Account switcher, view mode toggle (traditional/messaging)
- WebSocket push for live inbox updates

### Search (V5)
- FTS5 full-text search with snippet highlighting
- Filter chips: has:attachment, date ranges
- Dedicated search page with header search bar

### AI Intelligence (V6-V8)
- Automatic email classification: intent, priority, category
- Entity extraction: people, dates, amounts, topics, deadlines
- Thread summarization (lazy, cached)
- AI writing assist: rewrite, formal, casual, shorter, longer
- Natural language chat with email context via RAG
- Chat action proposals with confirmation flow
- AI feedback loop: user corrections improve future classifications

### Agent Connectivity (V9)
- Scoped API keys with 4 permission levels (read, draft, send, admin)
- Agent REST API: search, read messages/threads, create drafts, send
- Audit logging for all agent actions
- Trust indicators: SPF/DKIM/DMARC parsing, tracking pixel detection

### AI Memory Layer (V10)
- Memories MCP integration for semantic search
- Emails stored in vector store on ingest
- Chat RAG powered by semantic search with FTS5 fallback
- Semantic search toggle in search endpoint

### Infrastructure
- Session-token authentication (same-origin bootstrap)
- 11 integration tests + 81 unit tests
- Enhanced health endpoint (DB, Ollama, Memories status)
- Graceful shutdown (SIGINT + SIGTERM)
- Docker Compose with Ollama sidecar and healthchecks
- SQLite with FTS5, 4 migrations
