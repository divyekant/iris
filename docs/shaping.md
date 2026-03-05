---
shaping: true
---

# Iris — System Architecture Shaping

## Frame

### Source

User vision: "A new email client — current email clients are old and obsolete. The current world looks at email from a messaging app POV — same functionality but a different style and a lot more performative. AI FIRST — everyone wants AI in their emails but are afraid. If we build a self-deployable email webapp/client then they can connect it to local AI. Rethinking email: re-look at user workflows, data storage and access for AI capabilities, how any agent can connect to this system, how the entire piece moves. THIS IS A BIG PROJECT — start small."

"First is to define all existing capabilities — while we are building email, we can't go live without having current parity."

"Local AI is just connectors — we can add integration setup options so it's not a big deal."

### Problem

Email's data layer and workflow layer are stuck in 1990s design. No email client occupies the intersection of: self-deployable + AI-native data layer + messaging UX + any provider + open agent connectivity. Users want AI in email (82%) but don't trust how data is handled (81%). The privacy-preserving, AI-native email client doesn't exist.

### Outcome

A self-deployable email client where the data layer is designed for AI from day one, any agent can connect, the UX reimagines email workflows, and privacy is architectural — not policy.

---

## Requirements (R)

| ID | Requirement | Status |
|----|-------------|--------|
| **R0** | **Feature parity with existing email clients** | Core goal |
| R0.1 | Core email operations: compose, reply, forward, attachments, drafts, signatures, undo send, templates, scheduled send | Must-have |
| R0.2 | Inbox management: threading, labels/folders, stars, archive, snooze, mute, spam/block | Must-have |
| R0.3 | Search: full-text, operators, saved searches, filters/rules | Must-have |
| R0.4 | Organization: tasks, follow-up nudges, calendar integration, contacts | Must-have |
| R0.5 | Multi-account with unified inbox, send-as/alias support | Must-have |
| R0.6 | Rich text editing: bold/italic, lists, inline images, markdown compose | Must-have |
| R0.7 | Keyboard-first interaction with comprehensive shortcuts | Must-have |
| **R1** | **AI-native data layer** — storage, indexing, and access designed for AI queries from day one | Core goal |
| R1.1 | Structured email metadata extracted and indexed on ingest (entities, intent, sentiment, deadlines, relationships) | Must-have |
| R1.2 | Embedding-based semantic index alongside traditional full-text search | Must-have |
| R1.3 | Cross-thread knowledge graph: people, projects, decisions, topics linked across all email | Nice-to-have |
| R1.4 | Attachment content indexed (PDFs, docs, spreadsheets searchable) | Nice-to-have |
| **R2** | **Self-deployable** — users run their own instance with no cloud dependency | Core goal |
| R2.1 | Single-binary or Docker deployment — no complex infrastructure | Must-have |
| R2.2 | All data stored locally — email, indexes, AI models, config | Must-have |
| R2.3 | Works fully offline after initial sync | Must-have |
| R2.4 | Zero external service dependencies beyond the mail server itself | Must-have |
| **R3** | **Agent connectivity** — any AI agent can interact via standard protocols | Core goal |
| R3.1 | MCP server exposing email as tools (search, read, compose, send, label, archive) | Must-have |
| R3.2 | REST/GraphQL API with scoped authentication tokens | Must-have |
| R3.3 | Agent permission framework: read-only, draft-only, send-with-approval, autonomous | Must-have |
| R3.4 | Webhook triggers on email events with optional AI-powered semantic filters | Nice-to-have |
| R3.5 | Agent activity audit trail | Must-have |
| **R4** | **Reimagined UX** — messaging-style patterns, newsletter feed, inbox intelligence | Core goal |
| R4.1 | Opt-in messaging-style view alongside traditional email view (toggle, not forced) | Must-have |
| R4.2 | Newsletter/subscription feed view (HEY-style "The Feed") | Must-have |
| R4.3 | Screener for unknown senders (HEY-style approve/reject) | Nice-to-have |
| R4.4 | Smart categorization beyond Gmail's 5 tabs — dynamic, learning categories | Must-have |
| R4.5 | Subscription management dashboard with one-click unsubscribe | Nice-to-have |
| **R5** | **Privacy as architecture** — all processing local by default | Core goal |
| R5.1 | No telemetry, tracking, or data collection | Must-have |
| R5.2 | All AI inference runs locally — no data sent to external AI services unless user explicitly configures it | Must-have |
| R5.3 | Tracking pixel detection and blocking | Must-have |
| R5.4 | Email authentication visualization (SPF/DKIM/DMARC trust indicators) | Must-have |
| R5.5 | E2E encryption support (PGP/S/MIME) | Nice-to-have |
| **R6** | **Any email provider** — Gmail, Outlook, Yahoo, Fastmail, any IMAP server | Core goal |
| R6.1 | IMAP + SMTP as baseline protocol (covers all providers) | Must-have |
| R6.2 | OAuth2 per provider (Gmail, Outlook, Yahoo) | Must-have |
| R6.3 | JMAP as optional fast path for compatible servers | Nice-to-have |
| R6.4 | Provider-specific optimizations (Gmail API, Graph API) | Nice-to-have |
| **R7** | **Conversational interface** — chat with inbox, NL commands, briefings | Core goal |
| R7.1 | Chat panel for natural language queries about inbox state | Must-have |
| R7.2 | Email composition via conversational intent ("tell Sarah the project is delayed") | Must-have |
| R7.3 | Bulk operations via natural language ("archive all LinkedIn older than a month") | Nice-to-have |
| R7.4 | On-demand briefings and task extraction | Nice-to-have |
| **R8** | **Progressive intelligence** — starts simple, learns, adapts without setup | Core goal |
| R8.1 | Zero-config: works from day one with sensible defaults | Must-have |
| R8.2 | Learns user patterns over time (priority, categories, style, automation) | Must-have |
| R8.3 | AI connectors: plug in Ollama, llama.cpp, MLX, or cloud APIs | Must-have |
| R8.4 | Tiered model usage: small models (1-3B) for classification, medium (3-7B) for summarization, large (7B+) for generation | Must-have |

---

## Shapes

### A: Monolith Web App (Tauri + Rust Backend)

Desktop-first architecture. Tauri v2 shell wrapping a React/Svelte frontend. Rust backend handles IMAP sync, SQLite storage, local AI inference, and exposes APIs.

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **A1** | **Tauri v2 desktop shell** — Rust backend + webview frontend (proven by Velo) | |
| **A2** | **SQLite + FTS5 storage** — single DB for email, metadata, AI indexes, config (Velo's 33-table pattern) | |
| **A3** | **IMAP sync engine in Rust** — direct IMAP connection, IDLE for push, local cache | |
| **A4** | **Embedded AI runtime** — llama.cpp/MLX compiled into the Rust backend, models stored locally | |
| **A5** | **API layer** — HTTP server in the Tauri backend for MCP + REST access | |
| **A6** | **Single-binary distribution** — Tauri bundles into one installable file per platform | |

**Trade-offs:**
- (+) Fastest performance, native feel, full offline, single binary
- (+) Direct hardware access for GPU inference (MLX on Apple Silicon)
- (+) Proven pattern (Velo has 33-table SQLite schema working)
- (-) Desktop-only initially — no web access from other devices
- (-) Cross-platform builds complex (macOS/Windows/Linux)
- (-) No multi-device sync without additional server layer

---

### B: Web-First with Local Backend (Server + SPA)

Web-first architecture. A local server process (Rust or Node) handles sync, storage, and AI. The UI is a standard SPA (React/Svelte) served by the local server. Accessible from any browser on the local network.

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **B1** | **Local server process** — Rust (or Node) daemon running on user's machine, manages IMAP sync + SQLite + AI | |
| **B2** | **SPA frontend** — React or Svelte, served by the local server, accessible at localhost or LAN | |
| **B3** | **SQLite + FTS5 storage** — same proven data layer as A | |
| **B4** | **WebSocket bridge** — server pushes real-time updates to browser via WS (bridges IMAP IDLE) | |
| **B5** | **AI runtime as sidecar** — Ollama runs as a separate process, server calls it via REST | |
| **B6** | **API layer** — same server exposes MCP + REST for agents | |
| **B7** | **Docker deployment option** — entire stack in a single Docker compose for self-hosting | |

**Trade-offs:**
- (+) Web-first = user's primary ask; accessible from any device on LAN
- (+) Docker deployment simplifies self-hosting dramatically (97% of self-hosters use Docker)
- (+) AI as sidecar (Ollama) = simpler to manage, user can upgrade models independently
- (+) Multi-device: access from phone, tablet, other computers on same network
- (-) Requires a running server process (daemon management)
- (-) Slightly more latency than native (WebSocket hop)
- (-) Ollama sidecar = two processes to manage (but Docker compose handles this)

---

### C: Hybrid — Local Server + Optional Tauri Shell

Combines B's web-first architecture with an optional Tauri desktop wrapper. The core is the local server (B1-B7), but users can also install a Tauri app that embeds the frontend with native features (notifications, menu bar, dock icon).

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **C1** | **Local server core** — identical to B (Rust server, SQLite, IMAP sync, AI sidecar, API layer) | |
| **C2** | **SPA frontend** — works in any browser; the same codebase serves both web and Tauri | |
| **C3** | **Optional Tauri shell** — wraps the SPA for native desktop experience (notifications, tray, shortcuts) | |
| **C4** | **Docker deployment** — server + Ollama in Docker compose for self-hosting | |
| **C5** | **Progressive deployment** — start with Docker/web, add Tauri shell later if desired | |

**Trade-offs:**
- (+) Best of both worlds: web-first delivery + native desktop experience when wanted
- (+) Single frontend codebase serves both web and desktop
- (+) Progressive: ship web first (fastest to market), add desktop shell later
- (+) Docker + web covers the self-hosting audience immediately
- (-) More to maintain long-term (two deployment surfaces)
- (-) Tauri shell adds complexity but is optional — can defer indefinitely

---

## Fit Check

| Req | Requirement | Status | A | B | C |
|-----|-------------|--------|---|---|---|
| R0 | Feature parity with existing email clients | Core goal | ✅ | ✅ | ✅ |
| R1 | AI-native data layer | Core goal | ✅ | ✅ | ✅ |
| R2 | Self-deployable | Core goal | ✅ | ✅ | ✅ |
| R3 | Agent connectivity (MCP/API) | Core goal | ✅ | ✅ | ✅ |
| R4 | Reimagined UX | Core goal | ✅ | ✅ | ✅ |
| R5 | Privacy as architecture | Core goal | ✅ | ✅ | ✅ |
| R6 | Any email provider | Core goal | ✅ | ✅ | ✅ |
| R7 | Conversational interface | Core goal | ✅ | ✅ | ✅ |
| R8 | Progressive intelligence | Core goal | ✅ | ✅ | ✅ |

**Notes:**
- All three shapes satisfy all requirements architecturally — the differentiation is in delivery strategy, not capability.
- The real question is: **web-first or desktop-first?**

### Differentiating Factors (Beyond Fit Check)

| Factor | A (Tauri Monolith) | B (Web-First) | C (Hybrid) |
|--------|-------------------|---------------|------------|
| Time to first usable product | Slower (cross-platform builds) | Fastest (browser = universal) | Fast (web first, shell later) |
| User's stated preference | "Start with web app first" | Matches exactly | Matches with future native option |
| Self-hosting ease | Harder (per-platform binary) | Easy (Docker) | Easy (Docker) |
| Multi-device access | No (single machine) | Yes (LAN/network) | Yes (LAN/network) |
| Mobile access | No | Yes (responsive web) | Yes (responsive web) |
| Native feel | Best | Good (no native notifications) | Best (when shell installed) |
| AI integration simplicity | Complex (embedded runtime) | Simple (Ollama sidecar) | Simple (Ollama sidecar) |
| Distribution to users | App store / download | `docker compose up` | Both options |

---

## Recommendation: Shape B (Web-First with Local Backend)

**Rationale:**

1. **User explicitly said "start with a web app first"**
2. **Docker is the standard** — 97% of self-hosters use Docker
3. **Ollama as AI sidecar** is the simplest integration — users already know Ollama, can manage models independently, and it provides a clean REST API
4. **Multi-device from day one** — access from phone, tablet, other computers
5. **Fastest to first usable product** — no cross-platform build matrix
6. **Can always add Tauri shell later** (Shape C is just B + optional C3/C5) — this isn't a lock-in decision

Shape C is the long-term evolution, but starting with B is the right first move.

---

## Detail B: Concrete Parts

Expanding Shape B into concrete sub-parts that map to implementation.

| Part | Mechanism | Flag |
|------|-----------|:----:|
| **B1** | **Server Core** | |
| B1.1 | Rust HTTP server (Axum) — serves SPA static files + REST API on single port | |
| B1.2 | WebSocket server — real-time push to frontend (new mail, sync status, AI results) | |
| B1.3 | Background task scheduler — sync intervals, AI processing queue, cleanup jobs | |
| **B2** | **Frontend SPA** | |
| B2.1 | Inbox — category tabs, message list, view mode toggle (traditional/messaging), bulk actions | |
| B2.2 | Thread view — message list, AI summary header, trust indicators, quick reply | |
| B2.3 | Compose — rich text + markdown, AI assist (rewrite/tone/translate), attachments, schedule | |
| B2.4 | Chat panel — sidebar AI conversation, NL queries, email actions, briefings | |
| B2.5 | Search — keyword + semantic toggle, filters, answer extraction | |
| B2.6 | Newsletter feed — inline rendering, subscription management, one-click unsubscribe | |
| B2.7 | Settings — accounts, AI connectors, categories, shortcuts, API keys, theme | |
| **B3** | **Data Layer** | |
| B3.1 | SQLite database — accounts, messages, threads, contacts, labels, config tables | |
| B3.2 | FTS5 full-text index — bodies, subjects, attachment text | |
| B3.3 | Vector store — per-message embeddings for semantic search (sqlite-vec or hnswlib) | |
| B3.4 | AI metadata tables — entities, intent, sentiment, priority score, categories per message | |
| **B4** | **Sync Engine** | |
| B4.1 | IMAP connection manager — per-account connections, OAuth2 token refresh, connection pooling | |
| B4.2 | Initial sync — folder structure → headers → bodies (incremental, newest-first) | |
| B4.3 | IMAP IDLE listener — real-time new mail detection → triggers AI pipeline → WebSocket push | |
| B4.4 | SMTP send — queue outgoing, handle errors/retries, track delivery status | |
| B4.5 | Bidirectional flag sync — read/unread, labels, archive, delete synced both directions | |
| **B5** | **AI Pipeline** | |
| B5.1 | Ollama REST client — prompt dispatch, completion receipt, model health check | |
| B5.2 | Ingest processor — on new email: classify intent, extract entities/deadlines, compute embedding, score priority, assign category | |
| B5.3 | On-demand processor — user-triggered: summarize thread, draft reply, semantic search, NL query | |
| B5.4 | Model router — route tasks to appropriate model tier (1-3B → classify, 3-7B → summarize, 7B+ → generate) | |
| **B6** | **API Layer** | |
| B6.1 | REST API — CRUD messages/threads/labels, search, compose, send; OpenAPI spec | |
| B6.2 | MCP server — email as tools (search, read, compose, send, label, archive) for MCP clients | |
| B6.3 | Agent auth — scoped tokens with permission levels (read, draft, send-with-approval, autonomous) | |
| B6.4 | Webhook dispatcher — fire configurable events on email state changes | |
| B6.5 | Audit logger — tamper-resistant log of every API/MCP/webhook action | |
| **B7** | **Deployment** | |
| B7.1 | Docker Compose — iris-server + ollama in one compose file, shared volume for data | |
| B7.2 | Environment config — provider credentials, AI model selection, port, allowed origins | |
| B7.3 | Health endpoint — server, Ollama, IMAP connection, sync status, disk usage |
