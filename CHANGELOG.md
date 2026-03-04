# Changelog

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
