# Iris — Session Status

**Last updated:** 2026-03-03
**Pipeline:** explore ✓ → shape ✓ → plan ✓ → **build (V8 DONE)** → verify → review → finish → release

---

## What's Done

### Explore Phase (Complete)
6 research documents (~2,250 lines total):
- `docs/research/01-market-landscape.md` — market size, pain points, AI appetite
- `docs/research/02-competitor-analysis.md` — 15 competitors mapped
- `docs/research/03-technical-foundations.md` — protocols, local AI, architecture
- `docs/research/04-capability-audit.md` — 200+ features, 14 categories, 11 clients
- `docs/research/05-ai-features-brainstorm.md` — 73 AI features, 10 categories
- `docs/research/06-ui-design.md` — UI wireframes, Gmail & Superhuman comparison

### Shape Phase (Complete)
- `docs/shaping.md` — 9 requirements (R0-R8), 3 shapes (A/B/C), **Shape B selected** (Web-First with Local Backend), Detail B with 30+ concrete sub-parts
- `docs/breadboard.md` — System breadboard: 9 Places, 57 UI affordances, 63 code affordances, 17 data stores, 6 key data flows, fit check tracing every R to parts
- `docs/slices.md` — 9 vertical implementation slices (V1-V9), per-slice affordance tables, all 137 affordances assigned, R coverage matrix

### Plan Phase (Complete)
- `docs/plans/2026-03-02-v1-foundation.md` — 20-task implementation plan for V1: Foundation
- `docs/plans/2026-03-03-v2-email-reader.md` — 7-task implementation plan for V2: Email Reader
- `docs/plans/2026-03-03-v3-compose.md` — 9-task implementation plan for V3: Compose & Send
- `docs/plans/2026-03-03-v4-manage-inbox.md` — 9-task implementation plan for V4: Manage Inbox

### Build Phase — V1: Foundation (Complete)
11 commits on `main`, 48 files, ~5,100 lines added. 10 passing tests.

**Backend (Rust/Axum):**
- SQLite schema: accounts, messages (nullable AI columns), FTS5 full-text search, config key-value, migrations
- Models: Account + Message with full CRUD, pagination, soft delete
- HTTP API: health, accounts, messages (unified inbox), config/theme
- OAuth2: Gmail + Outlook flows (oauth2 v5 type-state pattern)
- WebSocket: broadcast hub with typed events (NewEmail, SyncStatus, SyncComplete)
- IMAP: XOAUTH2 connection, newest-first sync (100 msg batches), IDLE listener (29-min timeout, auto-reconnect)

**Frontend (Svelte 5 + TypeScript + Vite 7 + Tailwind CSS 4):**
- App shell: sidebar nav, header (search stub), content area
- Account setup: multi-step wizard (provider select → OAuth → manual config → syncing → success)
- Inbox: message list with unread dots, smart date formatting, attachment icons, sync status
- Settings: theme toggle (light/dark/system) with API persistence
- API client + WebSocket client with auto-reconnect

**Infrastructure:**
- Multi-stage Dockerfile (Rust builder → Node frontend → slim runtime)
- Docker Compose: iris-server + Ollama sidecar

### Build Phase — V2: Email Reader (Complete)
6 commits on `main`, 16 files changed, ~870 lines added. 19 total tests (9 new).

**Backend:**
- MIME parsing (mailparse): extracts text/plain, text/html, attachment metadata from multipart emails
- Thread ID generation: References > In-Reply-To > Message-ID header chain
- MessageDetail model with full body fields, get_by_id, list_by_thread
- Mark-as-read API endpoint
- Thread API: GET /threads/{id} returns full thread with participants

**Frontend:**
- EmailBody component: DOMPurify sanitization + sandboxed iframe rendering
- MessageCard: expandable message cards with body, recipients, attachments
- ThreadView page: thread header (subject + participants), chronological messages, reply/forward stubs (V3)
- Inbox click-through: row click navigates to thread view

### Build Phase — V3: Compose & Send (Complete)
8 commits on `main`, ~1,100 lines added. 26 total tests (7 new).

**Backend:**
- SMTP send via lettre 0.11: XOAUTH2 for Gmail/Outlook, password auth fallback
- Email builder: RFC 2822 messages, multipart HTML, In-Reply-To/References headers
- OAuth token refresh: checks expiry (60s buffer), refreshes via oauth2 crate
- Draft model: save/update/list/delete/finalize using existing is_draft column
- Send API: POST /api/send — token refresh, build email, SMTP send, store in Sent
- Draft CRUD: POST /api/drafts, GET /api/drafts, DELETE /api/drafts/{id}
- MessageDetail: added message_id field for reply threading

**Frontend:**
- ComposeModal: full compose overlay with new/reply/reply-all/forward modes
- Pre-populated fields: To, Cc, Subject, Body based on mode
- Reply quoting ("> " prefix), forward with separator block
- Auto-save drafts (3s debounce), Cmd+Enter to send, Escape to close
- Compose button in Inbox header, Reply/Reply All/Forward in ThreadView
- API client: send, drafts.list/save/delete methods

### Build Phase — V4: Manage Inbox (Complete)
8 commits on `main`, ~600 lines added. 30 total tests (4 new).

**Backend:**
- Batch update endpoint: PATCH /api/messages/batch — archive, delete, mark read/unread, star/unstar
- View mode config persistence: PUT /api/config/view-mode — traditional/messaging toggle
- Category filtering: messages list API supports ?category= param, filters by labels JSON column

**Frontend:**
- Category tabs: All/Primary/Updates/Social/Promotions tab bar in inbox header
- Message selection: checkboxes on message rows with selection highlighting
- Bulk action bar: appears on selection — Archive, Mark Read/Unread, Star, Delete, Clear
- Account switcher: sidebar lists accounts, filters inbox by selected account
- Thread actions: star, archive, mark unread, delete buttons in thread header
- View mode toggle: traditional/messaging button in inbox header, persists preference

### Build Phase — V5: Keyword Search (Complete)
4 commits on `main`, ~400 lines added. 31 total tests (1 new).

**Backend:**
- Search endpoint: GET /api/search — FTS5 MATCH with snippet highlighting (`<mark>` tags)
- Filters: has_attachment, date range (after/before), account_id
- Pagination: limit/offset with total count
- FTS5 query sanitization: terms wrapped in double quotes for safe literal matching

**Frontend:**
- Search page: debounced input (300ms), filter chips (has:attachment, Last 7/30/365 days)
- Results with highlighted snippets via `{@html}`, unread indicators, attachment icons
- Result click navigates to thread view
- Header search bar: click/focus navigates to /search route

### Build Phase — V6: AI Ingest (Complete)
7 commits on `main`, ~900 lines added. 40 total tests (9 new).

**Backend:**
- Ollama client: HTTP client with health check, model listing, generate completion (5s/120s timeouts)
- AI pipeline: single-prompt classification returning JSON (intent, priority, category, summary)
- JSON parsing: handles raw JSON, markdown code blocks, partial responses
- AI config endpoints: GET/PUT /api/config/ai, POST /api/config/ai/test
- Message AI metadata: update_ai_metadata function + AI fields on MessageDetail
- Sync integration: spawns background AI classification after each message insert
- WebSocket: AiProcessed event for real-time frontend notification
- Graceful degradation: skips AI if disabled or Ollama unavailable

**Frontend:**
- Settings: AI Processing section (enable toggle, Ollama URL + test, model picker, connection indicator)
- Inbox: priority color dots (urgent=red, high=orange, normal=green, low=gray), AI category pills
- API client: ai.getConfig, ai.setConfig, ai.testConnection methods

### Build Phase — V7: AI On-Demand (Complete)
2 commits on `main`, ~460 lines added. 49 total tests (9 new).

**Backend:**
- Thread summarize endpoint: POST /api/threads/{id}/summarize — lazy summary with caching
- AI assist endpoint: POST /api/ai/assist — text transformation (rewrite, formal, casual, shorter, longer)
- build_summary_prompt: concatenates thread messages with per-message body truncation (500 chars) and total cap (3000 chars)
- get_assist_system_prompt: maps action to Ollama system prompt, returns None for invalid actions

**Frontend:**
- Collapsible AI summary panel in ThreadView (shown for threads with >1 message)
- AI assist dropdown in ComposeModal footer (Improve writing, Make formal/casual, Make shorter, Expand)
- API client: threads.summarize, ai.assist methods

**Scoping note:** Semantic search (U40, U41) deferred — requires sqlite-vec embeddings infrastructure.

### Build Phase — V8: AI Chat (Complete)
2 commits on `main`, ~700 lines added. 54 total tests (5 new).

**Backend:**
- Migration 002: chat_messages table with session_id, role, citations, proposed_action
- POST /api/ai/chat: FTS5-based RAG context retrieval, Ollama conversation generation
- GET /api/ai/chat/{session_id}: conversation history retrieval
- POST /api/ai/chat/confirm: execute confirmed bulk actions (archive, delete, mark read/unread, star)
- Action proposal parsing from AI response (ACTION_PROPOSAL: JSON format)
- System prompt instructs AI to cite emails by ID and propose actions with confirmation

**Frontend:**
- ChatPanel: sliding right sidebar with conversation bubbles, suggestion chips, citation display
- Action confirmation UI: proposed actions shown with Confirm button
- Header: AI Chat toggle button
- AppShell: ChatPanel wired alongside main content area
- API client: ai.chat, ai.chatHistory, ai.chatConfirm methods

**Scoping note:** Newsletter Feed (P6) deferred. Semantic search (embeddings) deferred — FTS5 used for context retrieval.

### Key Decisions Made
- **Architecture:** Shape B — Rust (Axum) backend + SPA frontend + SQLite/FTS5 + Ollama sidecar + Docker Compose
- **Frontend:** Svelte 5 + TypeScript + Vite 7 + Tailwind CSS 4
- **AI:** Ollama as sidecar, tiered models (1-3B classify, 3-7B summarize, 7B+ generate)
- **Deployment:** Docker Compose (iris-server + Ollama)
- **Data:** SQLite + FTS5 + sqlite-vec for embeddings (vec added in later slice)

---

## What's Next

### Plan + Build V9: Agent Connectivity
V9 covers external agent integration:
- Agent API for programmatic inbox access
- Trust/security model for agent actions
- Webhook notifications

### After V9
All 9 slices complete. Polish, testing, deployment.

---

## Project Structure

```
iris/
├── README.md
├── Cargo.toml
├── Cargo.lock
├── Dockerfile
├── docker-compose.yml
├── migrations/
│   └── 001_initial.sql
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── smtp.rs       (SMTP send, email builder)
│   ├── ai/           (ollama client, classification pipeline)
│   ├── api/          (health, accounts, messages, config, compose, search, ai_config)
│   ├── auth/         (OAuth2 for Gmail/Outlook, token refresh)
│   ├── db/           (pool, migrations)
│   ├── imap/         (connection, sync, idle)
│   ├── models/       (account, message + drafts)
│   └── ws/           (hub, handler)
├── web/
│   ├── src/
│   │   ├── App.svelte
│   │   ├── components/   (AppShell, Sidebar, Header, inbox/, thread/, compose/)
│   │   ├── pages/        (Inbox, AccountSetup, Settings, ThreadView, Search)
│   │   └── lib/          (api.ts, ws.ts)
│   ├── vite.config.ts
│   └── package.json
└── docs/
    ├── shaping.md
    ├── breadboard.md
    ├── slices.md
    ├── SESSION-STATUS.md
    ├── research/       (6 documents)
    └── plans/
        ├── 2026-03-02-v1-foundation.md
        ├── 2026-03-03-v2-email-reader.md
        ├── 2026-03-03-v3-compose.md
        └── 2026-03-03-v4-manage-inbox.md
```
