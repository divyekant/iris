---
shaping: true
---

# Iris — Implementation Slices

Vertical slices for Shape B (Web-First with Local Backend). Each slice ends in demo-able UI. Sliced from the [system breadboard](breadboard.md).

---

## Slice Summary

| # | Slice | Mechanism | Demo |
|---|-------|-----------|------|
| V1 | Foundation | B1.1, B1.2, B3.1, B4.1-B4.3, B7.1-B7.3 | "docker compose up → add Gmail via OAuth → emails appear → new mail pushes live" |
| V2 | Read Email | B2.2 (basic) | "Click email → see thread messages rendered with attachments" |
| V3 | Write Email | B2.3 (basic), B4.4 | "Compose → send → undo send. Reply, forward." |
| V4 | Manage Inbox | B2.1 (full), B4.5 | "Messaging view toggle. Bulk archive. Multi-account. Bidirectional sync." |
| V5 | Keyword Search | B2.5 (keyword), B3.2 | "Search 'invoice' → highlighted results → click to open" |
| V6 | AI Ingest | B5.1, B5.2, B5.4, B3.3, B3.4 | "Email arrives → auto-categorized → priority badge → intent label on row" |
| V7 | AI On-Demand | B5.3, B2.2 (AI), B2.3 (AI), B2.5 (semantic) | "Thread AI summary. Compose AI assist. Semantic search + answer extraction." |
| V8 | Chat + Feed | B2.4, B2.6 | "Chat: 'summarize unread' → briefing. Newsletter feed: inline rendering + unsubscribe." |
| V9 | Agent API + Trust | B6.1-B6.5, privacy UI | "Trust badges on emails. Agent queries via MCP. Audit trail." |

---

## Progression Logic

```
V1 Foundation ──► V2 Read ──► V3 Write ──► V4 Manage
     │                                        │
     │              "Usable email client"◄─────┘
     │
     └──► V5 Search ──► V6 AI Ingest ──► V7 AI On-Demand ──► V8 Chat + Feed
                                                                    │
                              "AI-native email client"◄─────────────┘
                                         │
                                         └──► V9 Agent API + Trust
                                                      │
                                    "Full Iris"◄──────┘
```

**V1-V4** = usable email client (feature parity baseline)
**V5-V8** = AI-native intelligence layer
**V9** = agent connectivity + privacy features

---

## V1: Foundation — Account + Sync + Inbox List

**Mechanism:** B1.1, B1.2, B3.1, B4.1, B4.2, B4.3, B7.1, B7.2, B7.3

**Demo:** "Run `docker compose up`. Open browser. Add Gmail account via OAuth. Emails sync and appear in inbox list. New emails push live via WebSocket."

This is the hardest slice — it stands up the entire backend infrastructure. After V1, every subsequent slice adds features on a working foundation.

### Places Introduced

| # | Place | Scope |
|---|-------|-------|
| P1 | Inbox | Basic message list only (no categories, no bulk actions, no view toggle) |
| P7 | Account Setup | Provider selection, OAuth, IMAP config, sync progress |
| P8 | Settings | Theme toggle only (minimal settings shell) |
| P9 | Backend | Server core, data layer, sync engine, WebSocket |

### UI Affordances

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U2 | P1 | inbox | message list | render | — | — |
| U3 | P1 | message-row | message preview (sender, subject, snippet, time) | render | — | — |
| U4 | P1 | message-row | row click | click | → P2 (V2) | — |
| U11 | P1 | inbox | unread badge | render | — | — |
| U12 | P1 | inbox | sync status indicator | render | — | — |
| U48 | P7 | setup | provider selector (Gmail, Outlook, Yahoo, Fastmail, Other IMAP) | click | → N20 | — |
| U49 | P7 | setup | OAuth2 redirect button | click | → N21 | — |
| U50 | P7 | setup | IMAP/SMTP manual config fields | type | — | — |
| U51 | P7 | setup | connection test indicator | render | — | — |
| U52 | P7 | setup | initial sync progress bar | render | — | — |
| U57 | P8 | settings | theme toggle (light/dark/system) | click | → N26 | — |

### Code Affordances

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N20 | P7 | setup | `selectProvider(provider)` | call | — | → U49, U50 |
| N21 | P7 | setup | `startOAuth(provider)` | call | → N40 | → U51 |
| N26 | P8 | settings | `setTheme(theme)` | call | → S4 | → U57 |
| N27 | P9 | api | `GET /messages?account=` — list messages | call | → S5 | → U2, U3 |
| N40 | P9 | auth-service | `GET /auth/oauth/:provider` | call | → S11 | → N21 |
| N44 | P1 | ws-client | `onNewEmail(event)` — new email notification | observe | → N27 | → U11 |
| N45 | P1 | ws-client | `onSyncStatus(event)` — sync progress | observe | — | → U12 |
| N50 | P9 | imap-engine | IMAP connection manager | call | → S14 | — |
| N51 | P9 | imap-engine | `initialSync(account)` — download folder → headers → bodies | call | → S5 | → U52 |
| N53 | P9 | imap-engine | `idleListener(account)` — IMAP IDLE → on new mail → push | observe | → N51, → N44 | — |

### Data Stores

| # | Store | Description |
|---|-------|-------------|
| S4 | `theme` | UI theme preference |
| S5 | `messages` table | Core email store (headers, body, flags, labels) |
| S11 | `accounts` table | Account configs (provider, OAuth tokens, IMAP/SMTP) |
| S14 | Mail servers (external) | Gmail, Outlook, etc. |

### Key Decisions in V1

- **SQLite schema up front** — even though AI columns (intent, priority, embedding) come in V6, the messages table should include nullable AI columns from day one. Avoids migration pain later.
- **WebSocket from day one** — not polling. This is the real-time backbone that every later slice depends on.
- **Docker Compose** — iris-server + Ollama containers. Ollama sits idle until V6 but is present in the stack from the start.
- **App shell** — the SPA shell (sidebar nav, main content area, header) is built in V1, even though most nav items are disabled/empty. Avoids layout rework in later slices.

---

## V2: Read Email — Thread View

**Mechanism:** B2.2 (basic — no AI summary, no trust indicators yet)

**Demo:** "Click an email row in inbox. See the full thread with all messages rendered chronologically. Attachments listed. Navigate back to inbox."

### Places Introduced

| # | Place | Scope |
|---|-------|-------|
| P2 | Thread View | Basic — message list, body rendering, attachments. No AI summary, no trust badges yet. |

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U13 | P2 | thread-view | thread header (subject, participants) | render | — | — |
| U15 | P2 | thread-view | message list in thread | render | — | — |
| U16 | P2 | message-card | message body (sanitized HTML in sandboxed iframe) | render | — | — |
| U19 | P2 | message-card | attachment list | render | — | — |
| U22 | P2 | thread-view | "Reply" / "Reply All" / "Forward" buttons | click | → P3.1 (V3) | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N46 | P2 | ws-client | `onThreadUpdate(event)` — receive thread changes | observe | — | → U15 |

### Wires from V1

- U4 (row click) → P2 — now functional (was stub in V1)
- N27 (GET /messages) — extended to return full thread by threadId

### Key Decisions in V2

- **HTML sanitization** — sandboxed iframe for message body rendering. Security-critical from day one.
- **Reply/Forward buttons wire to P3.1** — but P3.1 doesn't exist until V3, so they're visible but disabled.

---

## V3: Write Email — Compose + Send

**Mechanism:** B2.3 (basic — no AI assist), B4.4

**Demo:** "Click compose. Write an email with rich text formatting. Attach a file. Send. See undo-send bar appear. Click undo before timer expires. Reply to a thread. Forward an email."

### Places Introduced

| # | Place | Scope |
|---|-------|-------|
| P3 | Compose | Full compose — rich editor, attachments, signatures, schedule, drafts. No AI assist yet. |
| P3.1 | Compose (Modal) | Modal overlay from P1 or P2 |

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U23 | P3 | compose | To / CC / BCC fields with autocomplete | type | → N7 | — |
| U24 | P3 | compose | subject field | type | — | — |
| U25 | P3 | compose | rich text editor (bold, italic, lists, inline images, markdown toggle) | type | — | — |
| U26 | P3 | compose | attachment drop zone | drop/click | → N8 | — |
| U28 | P3 | compose | send button | click | → N10 | — |
| U29 | P3 | compose | schedule send | click | → N11 | — |
| U30 | P3 | compose | save draft | click | → N12 | — |
| U31 | P3 | compose | signature selector | click | → N13 | — |
| U32 | P3 | compose | undo send bar (5-30s window) | click | → N14 | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N7 | P3 | compose | `contactAutocomplete(query)` | call | → N29 | → U23 |
| N8 | P3 | compose | `uploadAttachment(file)` | call | → N30 | → U26 |
| N10 | P3 | compose | `sendEmail(draft)` — queue for SMTP | call | → N32 | → U32 |
| N11 | P3 | compose | `scheduleSend(draft, datetime)` | call | → N33 | — |
| N12 | P3 | compose | `saveDraft(draft)` — local DB + IMAP drafts | call | → N34 | — |
| N13 | P3 | compose | `selectSignature(id)` | call | → S2 | → U25 |
| N14 | P3 | compose | `undoSend(messageId)` — cancel queued send | call | → N35 | → U32 |
| N29 | P9 | api | `GET /contacts?q=` — search contacts | call | → S6 | → N7 |
| N30 | P9 | api | `POST /attachments` — store blob | call | → S7 | → N8 |
| N32 | P9 | smtp-service | `POST /send` — queue for SMTP | call | → N49 | → N10 |
| N33 | P9 | scheduler | `POST /send/schedule` | call | → S8 | → N11 |
| N34 | P9 | api | `PUT /drafts` — save + sync to IMAP | call | → S5, → N47 | → N12 |
| N35 | P9 | smtp-service | `DELETE /send/:id` — cancel queued | call | → N49 | → N14 |
| N49 | P9 | smtp-engine | SMTP connection manager | call | → S14 | — |

### Data Stores (NEW)

| # | Store | Description |
|---|-------|-------------|
| S2 | `signatures` | User's configured email signatures |
| S6 | `contacts` table | Contact profiles |
| S7 | `attachments` blob store | Attachment files |
| S8 | `scheduled_sends` table | Queued future sends |

### Wires from V2

- U22 (Reply/Forward) → P3.1 — now functional
- U20 (quick reply) → N5 → N10 — wired through to send

### Code Affordances (ALSO NEW — wired from V2)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N5 | P2 | thread-view | `quickReply(threadId, body)` | call | → N10 | → U15 |
| N47 | P9 | sync-engine | `syncToServer(changes)` — push to IMAP | call | → N50 | — |
| U20 | P2 | thread-view | quick reply bar | type + click | → N5 | — |

---

## V4: Manage Inbox — Bulk Ops + View Toggle + Multi-Account

**Mechanism:** B2.1 (full), B4.5

**Demo:** "Toggle between Traditional and Messaging view. Select 5 emails, archive in bulk. Switch to second account — unified inbox shows both. Label changes sync back to Gmail."

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U1 | P1 | inbox | category tabs (Primary, Updates, Social — static for now) | click | → N1 | — |
| U5 | P1 | inbox | view mode toggle (Traditional / Messaging) | click | → N2 | — |
| U6 | P1 | inbox | bulk action bar (archive, delete, label, snooze) | click | → N3 | — |
| U7 | P1 | inbox | compose button | click | → P3.1 | — |
| U8 | P1 | inbox | search bar (focus opens P5) | click | → P5 (V5) | — |
| U9 | P1 | inbox | chat panel toggle | click | → P4 (V8) | — |
| U10 | P1 | inbox | account switcher | click | → N4 | — |
| U21 | P2 | thread-view | thread actions (archive, label, snooze, mute, delete) | click | → N6 | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N1 | P1 | inbox | `loadCategory(category)` | call | → N27 | → U2, U3 |
| N2 | P1 | inbox | `toggleViewMode(mode)` | call | → S1 | → U2 |
| N3 | P1 | inbox | `bulkAction(action, messageIds)` | call | → N28 | → U2 |
| N4 | P1 | inbox | `switchAccount(accountId)` | call | → N27 | → U2, U11 |
| N6 | P2 | thread-view | `threadAction(action, threadId)` | call | → N28 | → U21 |
| N28 | P9 | api | `PATCH /messages/batch` — bulk update | call | → S5, → N47 | → N3, N6 |

### Data Stores (NEW)

| # | Store | Description |
|---|-------|-------------|
| S1 | `viewMode` | Current view: traditional or messaging |

### Key Decisions in V4

- **Category tabs are static** — hard-coded Primary/Updates/Social/Promotions. Dynamic AI categories come in V6.
- **Messaging view** is a layout variant of the same data — bubble-style rendering of the same message list, not a separate data model.
- **Bidirectional sync** (B4.5) — archive in Iris → flags on IMAP server. Label on Gmail → shows in Iris. This is the reliability slice.

---

## V5: Keyword Search

**Mechanism:** B2.5 (keyword only), B3.2

**Demo:** "Type 'invoice from Amazon' in search. Filter by has:attachment and last 30 days. See results with highlighted matches. Click to open thread."

### Places Introduced

| # | Place | Scope |
|---|-------|-------|
| P5 | Search | Keyword search + filters. No semantic toggle, no answer extraction yet. |

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U38 | P5 | search | search input with operator autocomplete | type | → N17 | — |
| U39 | P5 | search | filter chips (date, sender, has:attachment, label, unread) | click | → N17 | — |
| U42 | P5 | search | results list (highlighted matches) | render | — | — |
| U43 | P5 | search | result row click | click | → P2 | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N17 | P5 | search | `keywordSearch(query, filters)` — FTS5 | call | → N37 | → U42 |
| N37 | P9 | search-service | `GET /search?q=&filters=` — FTS5 search | call | → S10 | → N17 |

### Data Stores (NEW)

| # | Store | Description |
|---|-------|-------------|
| S10 | `fts_messages` FTS5 index | Full-text search over bodies + subjects |

### Wires from V4

- U8 (search bar) → P5 — now functional

### Key Decisions in V5

- **FTS5 index built during V1 sync** — even though search UI comes in V5, the FTS5 index is populated as part of `initialSync` from V1. This avoids a re-index when search ships.
- **Operator autocomplete** — `from:`, `to:`, `has:attachment`, `is:unread`, `before:`, `after:` — matching Gmail's search operators for familiarity.

---

## V6: AI Ingest — Pipeline + Smart Categories

**Mechanism:** B5.1, B5.2, B5.4, B3.3, B3.4

**Demo:** "Open Ollama settings, connect to local instance. New email arrives — automatically classified as 'Action Required', priority badge (red) appears on inbox row, extracted deadline shows '2024-03-15'. Category tabs now show dynamic AI categories."

This is the slice where Iris stops being "another email client" and becomes AI-native.

### UI Affordances (MODIFIED)

| # | Change | Detail |
|---|--------|--------|
| U1 | ENHANCED | Category tabs become dynamic — AI-assigned categories, not just static Primary/Updates/Social |
| U3 | ENHANCED | Message row now shows: priority badge (color-coded), intent label, extracted deadline |
| U53 | NEW | P8: AI connector config (Ollama URL, model picker, test connection) |
| U54 | NEW | P8: Category manager (rename, reorder, add, remove dynamic categories) |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N22 | P8 | settings | `configureAI(ollamaUrl, model)` | call | → N41 | → U53 |
| N23 | P8 | settings | `updateCategories(config)` | call | → N42 | → U54 |
| N41 | P9 | ai-service | `PUT /config/ai` — save + test connection | call | → S12, → N48 | → N22 |
| N42 | P9 | api | `PUT /config/categories` — save category config | call | → S12 | → N23 |
| N48 | P9 | ollama-client | `generate(model, prompt)` — send to Ollama REST | call | → S15 | — |
| N52 | P9 | ai-pipeline | `ingestEmail(message)` — full AI processing | call | → N48, → S5, → S9, → S10 | — |
| N54 | P9 | ai-pipeline | `classifyIntent(message)` | call | → N48 | → S5 |
| N55 | P9 | ai-pipeline | `extractEntities(message)` | call | → N48 | → S5 |
| N56 | P9 | ai-pipeline | `computeEmbedding(message)` | call | → N48 | → S9 |
| N57 | P9 | ai-pipeline | `scorePriority(message)` — Eisenhower scoring | call | → N48 | → S5 |
| N58 | P9 | ai-pipeline | `assignCategory(message)` | call | → N48 | → S5 |

### Data Stores (NEW)

| # | Store | Description |
|---|-------|-------------|
| S9 | `embeddings` vector store | Per-message embeddings for semantic search |
| S12 | `config` table | App configuration (AI connectors, categories) |
| S15 | Ollama (external) | AI inference sidecar |

### Wires Modified

- N53 (IDLE listener) → now also triggers N52 (ingestEmail) on new mail arrival
- N51 (initialSync) → now triggers N52 for each synced message (background processing queue)

### Key Decisions in V6

- **Graceful degradation** — if Ollama is not running or not configured, everything from V1-V5 still works. AI columns stay null. No category tabs, no priority badges — just plain email.
- **Background queue** — AI ingest is async. Email appears in inbox immediately; AI metadata populates seconds later via WebSocket update.
- **Backfill** — on first Ollama connection, offer to process existing emails in background. Non-blocking.

---

## V7: AI On-Demand — Summary, Assist, Semantic Search

**Mechanism:** B5.3, B2.2 (AI features), B2.3 (AI assist), B2.5 (semantic search)

**Demo:** "Open a long 15-message thread — see collapsible AI summary at top. In compose, click AI assist — rewrite in formal tone. Toggle semantic search, type 'what did John say about the budget?', see extracted answer above results."

### UI Affordances (NEW / ENHANCED)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U14 | P2 | thread-view | AI summary (collapsible) | render | — | — |
| U27 | P3 | compose | AI assist button (rewrite, tone, translate, suggestions) | click | → N9 | — |
| U40 | P5 | search | semantic search toggle | click | → N18 | — |
| U41 | P5 | search | answer extraction panel (direct answer above results) | render | — | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N9 | P3 | compose | `aiAssist(action, content)` — rewrite/tone/translate | call | → N31 | → U25 |
| N18 | P5 | search | `semanticSearch(query)` — embedding search | call | → N38 | → U41, U42 |
| N31 | P9 | ai-service | `POST /ai/assist` — writing assistance | call | → N48 | → N9 |
| N38 | P9 | search-service | `GET /search/semantic?q=` — similarity search | call | → S9, → N48 | → N18 |
| N59 | P9 | ai-pipeline | `summarizeThread(threadId)` | call | → N48, → S5 | → U14 |

### Key Decisions in V7

- **Thread summary is lazy** — computed on first view of a thread, then cached. Not pre-computed for all threads.
- **Answer extraction** uses the same RAG pipeline as chat (V8) but returns a single answer + source citations. This is the lightweight version of chat.
- **AI assist actions**: rewrite (improve clarity), tone (formal/casual/friendly), translate (detect → target), suggest (complete sentence). Each is a single Ollama call.

---

## V8: Chat Panel + Newsletter Feed

**Mechanism:** B2.4, B2.6

**Demo:** "Open chat panel. Ask 'summarize my unread from today'. Get briefing with email citations you can click. Ask 'archive all LinkedIn older than a month' — see confirmation before executing. Switch to Feed tab — newsletters rendered inline like an RSS reader. Unsubscribe from a noisy sender."

### Places Introduced

| # | Place | Scope |
|---|-------|-------|
| P4 | Chat Panel | Full — conversation history, NL queries, action confirmation, email citations |
| P6 | Newsletter Feed | Full — inline rendering, subscription sidebar, unsubscribe |

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U33 | P4 | chat-panel | conversation history | render | — | — |
| U34 | P4 | chat-panel | chat input | type + enter | → N15 | — |
| U35 | P4 | chat-panel | suggested actions (briefing, tasks, search) | click | → N15 | — |
| U36 | P4 | chat-panel | AI response with email citations | render | — | — |
| U37 | P4 | chat-panel | action confirmation ("Archive 47 emails?") | click | → N16 | — |
| U44 | P6 | feed | newsletter list (scrollable, inline-rendered) | render | — | — |
| U45 | P6 | feed | subscription sidebar (sources, open rates) | render | — | — |
| U46 | P6 | feed | unsubscribe button per source | click | → N19 | — |
| U47 | P6 | feed | newsletter item (rendered content, not just subject) | render | — | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N15 | P4 | chat-panel | `chatQuery(message)` — NL query with inbox context | call | → N36 | → U33, U36 |
| N16 | P4 | chat-panel | `confirmAction(action)` — execute confirmed action | call | → N28 | → U37 |
| N19 | P6 | feed | `unsubscribe(senderId)` — trigger unsubscribe flow | call | → N39 | → U46 |
| N36 | P9 | ai-service | `POST /ai/chat` — conversational AI with RAG | call | → N48, → S5, → S9 | → N15 |
| N39 | P9 | subscription-service | `POST /unsubscribe` — List-Unsubscribe header | call | → S5 | → N19 |

### Wires from V4

- U9 (chat toggle) → P4 — now functional
- Newsletter feed accessible via sidebar nav (new tab in P1)

### Key Decisions in V8

- **Chat is RAG over your inbox** — every query embeds the user's message, retrieves relevant emails from vector store (S9), passes them as context to Ollama. No external data.
- **Action confirmation is mandatory** — chat proposes actions ("I'll archive 47 emails"), user must click confirm. No autonomous execution from chat.
- **Newsletter detection** uses `List-Unsubscribe` header and sender heuristics from V6 category assignment. Emails classified as "newsletter" auto-route to Feed.
- **Unsubscribe** fires List-Unsubscribe-Post or opens List-Unsubscribe URL. Iris handles RFC 8058.

---

## V9: Agent API + Trust Indicators

**Mechanism:** B6.1-B6.5, privacy UI

**Demo:** "Open a suspicious email — see green SPF/DKIM/DMARC trust badge. Open a marketing email — see 'tracking pixel blocked' alert. Go to Settings → API Keys → create a read-only key. External agent uses MCP to search inbox and draft a reply. Check audit log — every agent action logged."

### UI Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| U17 | P2 | message-card | trust indicator badge (SPF/DKIM/DMARC) | render | — | — |
| U18 | P2 | message-card | tracking pixel alert | render | — | — |
| U55 | P8 | settings | keyboard shortcut customizer | click | → N24 | — |
| U56 | P8 | settings | API key management (create, revoke, permissions) | click | → N25 | — |

### Code Affordances (NEW)

| # | Place | Component | Affordance | Control | Wires Out | Returns To |
|---|-------|-----------|------------|---------|-----------|------------|
| N24 | P8 | settings | `updateShortcuts(bindings)` | call | → S3 | → U55 |
| N25 | P8 | settings | `manageApiKey(action, config)` | call | → N43 | → U56 |
| N43 | P9 | auth-service | `POST /api-keys` — create/revoke scoped tokens | call | → S13 | → N25 |
| N60 | P9 | mcp-server | MCP tool handlers (search, read, compose, send, label, archive) | call | → S5, → N49, → N47 | — |
| N61 | P9 | api-gateway | Token validation + permission check | call | → S13 | — |
| N62 | P9 | webhook-service | `dispatchWebhook(event)` | call | → S16 | — |
| N63 | P9 | audit-service | `logAction(agent, action, scope)` | call | → S17 | — |

### Data Stores (NEW)

| # | Store | Description |
|---|-------|-------------|
| S3 | `shortcuts` | Keyboard shortcut bindings |
| S13 | `api_keys` table | Scoped agent tokens with permission levels |
| S16 | `webhooks` table | Configured webhook URLs and event filters |
| S17 | `audit_log` table | Tamper-resistant log of all agent/API actions |

### Key Decisions in V9

- **Trust badges** parse email headers that are already stored — SPF/DKIM/DMARC results from the `Authentication-Results` header. No external lookups needed.
- **Tracking pixel detection** — scan HTML body for 1x1 images and known tracking domains. Block loading. Show alert.
- **MCP server** follows the Model Context Protocol spec — email exposed as tools that any MCP-compatible agent (Claude, etc.) can use.
- **Permission levels**: read-only (search + read), draft-only (+ compose), send-with-approval (+ send but requires user confirmation), autonomous (full access). Sane default: read-only.
- **Audit trail** is append-only. Every agent action logged with: agent ID, action, scope, timestamp, request/response hash.

---

## Affordance Coverage Check

Every affordance from the breadboard assigned to exactly one slice:

| Slice | UI Affordances | Code Affordances | Data Stores |
|-------|---------------|-----------------|-------------|
| V1 | U2, U3, U4, U11, U12, U48-U52, U57 | N20, N21, N26, N27, N40, N44, N45, N50, N51, N53 | S4, S5, S11, S14 |
| V2 | U13, U15, U16, U19, U22 | N46 | — |
| V3 | U20, U23-U26, U28-U32 | N5, N7, N8, N10-N14, N29, N30, N32-N35, N47, N49 | S2, S6, S7, S8 |
| V4 | U1, U5-U10, U21 | N1-N4, N6, N28 | S1 |
| V5 | U38, U39, U42, U43 | N17, N37 | S10 |
| V6 | U53, U54 (+ U1 enhanced, U3 enhanced) | N22, N23, N41, N42, N48, N52, N54-N58 | S9, S12, S15 |
| V7 | U14, U27, U40, U41 | N9, N18, N31, N38, N59 | — |
| V8 | U33-U37, U44-U47 | N15, N16, N19, N36, N39 | — |
| V9 | U17, U18, U55, U56 | N24, N25, N43, N60-N63 | S3, S13, S16, S17 |

**Totals:** 57 UI + 63 Code + 17 Data Stores = all accounted for.

---

## R Coverage by Slice

| Req | First Addressed | Fully Satisfied |
|-----|----------------|----------------|
| R0.1 | V1 (inbox), V3 (compose/send) | V4 (all core ops) |
| R0.2 | V4 (threading, labels, archive, snooze) | V4 |
| R0.3 | V5 (keyword search) | V7 (+ semantic) |
| R0.4 | V6 (AI task extraction) | V8 (chat briefings) |
| R0.5 | V4 (multi-account, account switcher) | V4 |
| R0.6 | V3 (rich text editor) | V3 |
| R0.7 | V9 (shortcut customizer) | V9 |
| R1.1 | V6 (ingest: classify, extract, score) | V6 |
| R1.2 | V6 (embeddings) + V7 (semantic search) | V7 |
| R2.1 | V1 (Docker Compose) | V1 |
| R2.2 | V1 (SQLite local) | V1 |
| R2.3 | V1 (local data) | V1 |
| R2.4 | V1 (mail server only external dep) | V1 |
| R3.1 | V9 (MCP server) | V9 |
| R3.2 | V1 (REST API — grows each slice) | V9 (full OpenAPI) |
| R3.3 | V9 (agent permissions) | V9 |
| R3.5 | V9 (audit trail) | V9 |
| R4.1 | V4 (messaging view toggle) | V4 |
| R4.2 | V8 (newsletter feed) | V8 |
| R4.4 | V6 (AI categories) | V6 |
| R5.1 | V1 (no telemetry by design) | V1 |
| R5.2 | V6 (Ollama local) | V6 |
| R5.3 | V9 (tracking pixel detection) | V9 |
| R5.4 | V9 (trust badges) | V9 |
| R6.1 | V1 (IMAP + SMTP) | V3 (send works) |
| R6.2 | V1 (OAuth2 setup) | V1 |
| R7.1 | V8 (chat panel) | V8 |
| R7.2 | V8 (chat → compose) | V8 |
| R8.1 | V1 (works without AI) | V6 (sensible AI defaults) |
| R8.2 | V6 (categories adapt) | V6+ |
| R8.3 | V6 (Ollama connector) | V6 |
| R8.4 | V6 (model router) | V6 |

**Nice-to-haves deferred:** R1.3 (knowledge graph), R1.4 (attachment content), R4.3 (screener), R4.5 (subscription dashboard — partially in V8), R5.5 (E2E encryption), R6.3 (JMAP), R6.4 (provider APIs), R7.3 (bulk NL ops — partially in V8), R7.4 (briefings — partially in V8), R3.4 (webhooks — in V9).
