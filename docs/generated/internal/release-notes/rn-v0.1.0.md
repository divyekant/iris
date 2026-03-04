---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
release: v0.1.0
---

# Release Notes: Iris v0.1.0

**Release Date**: 2026-03-04
**Commits**: 63
**Tests**: 78 passing

This is the initial release of Iris, an AI-native, local-first email client. Everything in this release is new.

---

## V1-V4: Functional Email Client

### Email Accounts (V1)

- OAuth2 authentication for Gmail and Outlook with automatic IMAP/SMTP configuration.
- Manual IMAP/SMTP account setup with app passwords.
- Account CRUD API (list, create, get, delete).
- OAuth token refresh with 60-second expiration buffer.
- Provider-specific userinfo fetching (Google userinfo API, Microsoft Graph API).

### Email Sync (V1)

- IMAP sync engine with TLS connection and XOAUTH2 SASL authentication.
- Initial sync fetches the newest 100 messages from INBOX.
- IMAP IDLE push notifications with 29-minute timeout per RFC 2177.
- Exponential backoff on IDLE connection failures (30s initial, 15-min cap).
- WebSocket broadcast for sync progress, new emails, and sync completion events.

### Email Reading (V2)

- MIME parsing via `mailparse` for text/plain, text/html, and attachment metadata extraction.
- Thread ID extraction from References (first message-id) and In-Reply-To headers.
- MessageDetail model with full message content and metadata.
- Thread API returns all messages in a conversation, chronologically ordered.
- Email HTML rendering via DOMPurify sanitization + sandboxed iframe.

### Compose and Send (V3)

- Email composition via `lettre` with XOAUTH2 SMTP for Gmail and Outlook.
- Multipart/alternative messages (text + HTML) and plain text messages.
- Reply, reply-all, and forward with proper In-Reply-To and References headers.
- Draft CRUD (save, list, delete) with auto-save support.
- Sent messages stored locally with "Sent" folder designation.

### Inbox Management (V4)

- Batch actions: archive, delete, mark read/unread, star/unstar (up to 1000 messages per batch).
- Category tabs: Primary, Updates, Social, Promotions (filter by AI-assigned category).
- Account switcher for filtering by account or viewing unified inbox.
- View mode toggle (compact/comfortable) persisted in config.
- Message selection with bulk action toolbar.

## V5: Search

### Keyword Search

- FTS5 full-text search index on message_id, subject, body_text, from_address, from_name.
- Porter stemming with unicode61 tokenization for English-language search.
- FTS5 snippet highlighting with `<mark>` tags (40-token snippets).
- Search filters: has:attachment, date range (after/before), account scoping.
- Automatic FTS5 sync via INSERT/UPDATE/DELETE triggers on the messages table.

### Semantic Search

- Toggle between keyword (FTS5) and semantic (Memories) search modes.
- Hybrid BM25+vector search via Memories MCP client.
- Automatic fallback from semantic to FTS5 when Memories is unavailable.

## V6-V8: AI Features

### AI Classification (V6)

- Single-prompt classification pipeline via local Ollama LLM.
- Extracts: intent (6 types), priority_score (0-1), priority_label (4 levels), category (7 types), summary, entities (people, dates, amounts, topics), and deadline.
- Background processing during sync with semaphore-limited concurrency (max 4 tasks).
- JSON extraction handles raw JSON, markdown code blocks, and mixed output.
- AI configuration endpoints for enable/disable toggle, model selection, and connection testing.
- Priority badges and category pills in the inbox UI.

### AI Feedback Loop (V6)

- User correction API for category, priority_label, and intent fields.
- Corrections stored in ai_feedback table with original and corrected values.
- Feedback patterns (2+ occurrences) automatically appended to classification prompts.
- Feedback statistics endpoint showing correction counts and common patterns.

### Thread Summarization (V7)

- On-demand thread summary generation via Ollama.
- Lazy evaluation with caching in the first message's ai_summary column.
- Prompt construction with per-message body truncation (500 chars) and total cap (3000 chars).
- Collapsible summary panel in the ThreadView UI.

### Writing Assist (V7)

- Five rewrite actions: rewrite, formal, casual, shorter, longer.
- Input content cap at 50,000 characters.
- AI assist dropdown integrated into the ComposeModal.

### AI Chat (V8)

- Natural language conversation interface for email queries.
- RAG context retrieval via Memories semantic search with FTS5 fallback.
- Citation system linking AI responses to specific email messages.
- Action proposals (archive, delete, mark read/unread, star) with user confirmation.
- Session persistence in chat_messages table.
- Suggestion chips and conversation history.

## V9: Agent Connectivity

### Scoped API Keys

- API key generation: `iris_` prefix + 32 hex chars, SHA-256 hashed for storage.
- Four permission levels: read_only, draft_only, send_with_approval, autonomous.
- Optional account scoping to restrict agent access to a single email account.
- Key management UI in Settings (create, list, revoke).

### Agent REST API

- Search: `GET /api/agent/search` (FTS5 search with account scoping).
- Read: `GET /api/agent/messages/{id}`, `GET /api/agent/threads/{id}`.
- Draft: `POST /api/agent/drafts` (create drafts programmatically).
- Send: `POST /api/agent/send` (full SMTP send with token refresh).
- Bearer token authentication via `Authorization` header.

### Audit Logging

- All agent actions logged with key_id, action, resource, details, and status.
- Audit log viewable in Settings with filtering by API key and pagination.

### Trust Indicators

- SPF, DKIM, and DMARC status parsing from Authentication-Results headers.
- Tracking pixel detection (tiny 1x1 images + 16 known tracker domains).
- TrustBadge UI component showing email authentication status.

## V10: Semantic Memory

### Memories Integration

- MemoriesClient HTTP client for the Memories MCP server (localhost:8900).
- Email content stored in Memories during sync with source tagging: `iris/{account}/messages/{id}`.
- Body truncation to 4000 characters for embedding.
- Semantic search endpoint with `?semantic=true` toggle.
- Chat RAG replaced with Memories semantic search (FTS5 fallback retained).
- Memories health indicator in Settings.

## Infrastructure

### Authentication

- Per-startup random 64-char hex session token.
- Same-origin bootstrap via Sec-Fetch-Site validation.
- X-Session-Token header for all protected API requests.
- WebSocket authentication via query parameter token.

### Database

- SQLite with WAL mode, foreign keys, 5-second busy timeout.
- r2d2 connection pool (max 10 connections).
- 4 sequential migrations: initial schema, chat, agent, ai_feedback.
- 8 tables: accounts, messages, fts_messages, config, chat_messages, api_keys, audit_log, ai_feedback.
- Plus schema_version for migration tracking.

### Deployment

- Docker Compose with iris server and Ollama sidecar.
- Health checks on both containers.
- Graceful shutdown on SIGINT/SIGTERM.
- SPA static file serving with fallback to index.html.
- CORS configured for localhost development origins.

### Testing

- 78 unit tests covering: database operations, MIME parsing, thread ID extraction, AI JSON parsing, trust indicators, tracking pixels, API key management, audit logging, SMTP building, chat prompt construction, and more.

---

## Known Limitations

- Initial sync limited to newest 100 messages per account.
- Attachment content not stored (metadata only).
- Batch actions (archive, delete, etc.) are local-only and not synced to IMAP.
- Password-based accounts store credentials in plaintext.
- No email search by attachment filename.
- No mechanism for emptying trash or permanent deletion.
- Summary cache has no invalidation on new messages.
- Chat sessions have no expiration or cleanup.
