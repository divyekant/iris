# Iris Master Feature List

> Single source of truth for all features to build. Check items off as they ship.
>
> **Sources:** Capability Audit (04), AI Brainstorm (05), E2E Gap Fixes, Innovation Gaps
>
> **Last updated:** 2026-03-13

---

## Wave 1 — Feature Parity (Must-Have for Launch)

Core features every email client has. Without these, Iris isn't a real email client.

### 1A. Critical Bugs (from E2E gaps)

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 1 | Fix date display (epoch seconds → milliseconds) | XS | **done** |
| 2 | Decode RFC 2047 encoded subjects & sender names | S | **done** |
| 3 | Force white background for email body iframe | XS | **done** |
| 4 | Diagnose and fix email send (SMTP error surfacing) | S | **done** |

### 1B. Core Email Operations

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 5 | Attachments — send, receive, drag-and-drop | M | **done** |
| 6 | Inline attachment preview (PDF, images) | M | **done** |
| 7 | Rich text compose — font family, size, color | M | **done** |
| 8 | Undo send (5–30s configurable delay) | S | **done** |
| 9 | Signatures — multiple, per-account, rich HTML | S | **done** |
| 10 | Scheduled send | S | **done** |
| 11 | Templates / Canned responses | S | **done** |

### 1C. Inbox Management

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 12 | Folder navigation — Sent, Drafts, Starred, Archive, Trash | M | **done** |
| 13 | Stars / Flags (toggle per message) | S | **done** (quick action) |
| 14 | Labels / Tags — colored, multiple per message | M | **done** |
| 15 | Snooze (return email at specified time) | M | **done** |
| 16 | Spam report + Block sender | S | **done** |
| 17 | Mute thread | S | **done** |
| 18 | Archive vs Delete (distinct actions in UI) | XS | **done** |

### 1D. Search & Filtering

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 19 | Search operators — from:, to:, subject:, is:unread, AND/OR | M | **done** |
| 20 | Saved searches / Smart folders | M | **done** |
| 21 | Filters / Rules — auto-label, auto-archive, auto-delete | L | **done** |

### 1E. UI Essentials

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 22 | Install lucide-svelte, replace emoji icons | S | **done** |
| 23 | Inbox pagination (25 per page) | S | **done** |
| 24 | Quick actions on message row hover (archive, trash, star) | S | **done** |
| 25 | Refresh button + 60s background polling | XS | **done** |
| 26 | Keyboard shortcuts (j/k navigate, e archive, # delete, r reply, c compose) | M | **done** |
| 27 | New email toast notification + nav unread badge | S | **done** |
| 28 | Fix checkbox + unread dot spacing | XS | **done** |
| 29 | Improve compose button states (hover/active/disabled) | XS | **done** |
| 30 | Improve search UX (Cmd+K, empty state, result count) | S | **done** |
| 31 | Make chat panel resizable (drag handle, localStorage) | S | **done** |
| 32 | Gmail labels display as colored pills | S | **done** |

### 1F. Multi-Account & Identity

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 33 | Send-as / Alias support | M | **done** |

### 1G. Notifications & Sync

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 34 | Desktop notifications (Notification API) | S | **done** |
| 35 | Per-account notification control | S | **done** |

### 1H. AI Polish

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 36 | AI reprocess button for untagged messages | S | **done** |

**Wave 1 Total: 36 items**

---

## Wave 2 — Differentiation (Makes Iris Special)

Features that set Iris apart from Gmail/Outlook/Superhuman. Mix of innovation gaps and high-value AI.

### 2A. Innovation Gaps (Industry Firsts)

| # | Feature | Source | Effort | Status |
|---|---------|--------|--------|--------|
| 37 | Draft version history (no client has this) | Innovation #1 | M | **done** |
| 38 | Thread-level private notes (only HEY) | Innovation #2 | M | **done** |
| 39 | Newsletter feed view (only HEY, Shortwave) | Innovation #4 | L | pending |
| 40 | Subscription management dashboard (nobody) | Innovation #6 | L | pending |
| 41 | Search in attachment content — PDFs, docs (only Gmail/Outlook) | Innovation #11 | L | pending |
| 42 | Bounce/redirect on web (rare) | Innovation #12 | S | **done** |

### 2B. Inbox Intelligence (AI — Automatic)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 43 | Intent detection (action request, FYI, sales, social) | 1.4 | M | **done** |
| 44 | "Needs Reply" detection + queue | 1.8 | M | **done** |
| 45 | Deadline extraction → task creation | 1.5 | M | **done** |
| 46 | Sentiment analysis on incoming email | 1.6 | S | **done** |
| 47 | VIP auto-detection from behavior patterns | 1.10 | M | **done** |
| 48 | Relationship-aware prioritization | 1.3 | M | **done** |
| 49 | Thread importance decay over time | 1.7 | S | **done** |
| 50 | Duplicate/related thread clustering | 1.9 | L | pending |

### 2C. Composing & Writing (AI)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 51 | Draft from intent (plain English → full email) | 2.1 | M | **done** |
| 52 | Multi-option reply (3 tones: formal/casual/brief) | 2.2 | M | **done** |
| 53 | Context-aware autocomplete (knows thread history) | 2.5 | L | **done** |
| 54 | Smart CC/BCC suggestions | 2.7 | M | **done** |
| 55 | Subject line generation/improvement | 2.8 | S | **done** |
| 56 | Grammar & tone check (before-send) | 2.10 | M | **done** |
| 57 | Multi-language compose + translate | 2.4 | M | **done** |
| 58 | Markdown compose support | Audit | S | **done** |

### 2D. Chat & Agent Enhancements (AI)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 59 | On-demand briefing ("brief me on today") | 4.2 | S | **done** |
| 60 | Task extraction from email ("what am I supposed to do?") | 4.3 | M | **done** |
| 61 | Relationship intelligence queries | 4.4 | M | **done** |
| 62 | Bulk operations via chat ("archive all from LinkedIn") | 4.7 | M | **done** |
| 63 | Email composition via chat | 4.6 | S | **done** |

### 2E. Automation & Workflows (AI)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 64 | Auto-archive patterns (learn what user always archives) | 5.2 | M | pending |
| 65 | Content-based follow-up reminders | 5.3 | M | **done** |
| 66 | Newsletter digest generation (consolidate into 1 summary) | 5.6 | M | pending |
| 67 | Subscription audit (surface never-opened subscriptions) | 5.7 | S | **done** |
| 68 | One-click unsubscribe | Audit | S | **done** |
| 69 | Template auto-generation from repeated patterns | 2.9 | M | pending |
| 70 | Smart notification routing (urgent → push, FYI → digest) | 5.8 | M | pending |

### 2F. Security & Trust (AI)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 71 | Contextual phishing detection | 6.1 | L | pending |
| 72 | Social engineering detection | 6.2 | M | **done** |
| 73 | Impersonation detection (lookalike domains) | 6.3 | S | **done** |
| 74 | Link safety with context | 6.4 | M | **done** |
| 75 | Data leak prevention (before-send scan) | 6.5 | M | **done** |
| 76 | Privacy report (who tracks you, HEY-style) | Audit | M | **done** |

### 2G. Contact Intelligence (AI)

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 77 | Auto-generated contact profiles | 7.1 | L | pending |
| 78 | Relationship strength scoring | 7.2 | M | **done** |
| 79 | Key topics per contact | 7.4 | S | **done** |
| 80 | Response time patterns | 7.5 | S | **done** |

### 2H. Agent Connectivity Enhancements

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 81 | MCP Server (expose email as tools via MCP protocol) | 8.1 | L | pending |
| 82 | Webhook triggers on email events | 8.4 | M | pending |
| 83 | Structured data extraction API | 8.5 | M | pending |

### 2I. Productivity & Insights

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 84 | Email analytics dashboard | 9.1 | L | pending |
| 85 | Communication health reports | 9.2 | M | pending |
| 86 | Follow-up if no reply (Superhuman-style) | Audit | M | pending |
| 87 | Email effectiveness scoring (before-send) | 9.6 | M | pending |

**Wave 2 Total: 51 items**

---

## Wave 3 — Advanced / Blue-Sky (Post-Launch)

Aspirational features. Build only after Waves 1-2 are solid.

### 3A. Advanced AI

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 88 | Writing style learning (match user's voice) | 2.3 | L | pending |
| 89 | Cross-thread knowledge graph | 3.3 | XL | pending |
| 90 | People-centric search ("everything with Sarah about budget") | 3.5 | L | pending |
| 91 | Temporal reasoning ("emails from around v2 launch") | 3.6 | M | pending |
| 92 | Organizational mapping (infer org chart from CC patterns) | 7.6 | L | pending |
| 93 | CRM-like features (deal/project tracking from email) | 7.7 | XL | pending |
| 94 | Evolving auto-categorization (categories emerge over time) | 5.1 | L | pending |
| 95 | Proactive suggestion engine (AI-initiated optimizations) | 9.4 | L | pending |

### 3B. Automation

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 96 | Meeting scheduling negotiation (AI handles back-and-forth) | 5.9 | XL | pending |
| 97 | Out-of-office intelligence (AI handles email while away) | 5.10 | L | pending |
| 98 | Auto-draft for routine emails | 5.5 | M | pending |
| 99 | SLA monitoring (response time tracking) | 5.4 | M | pending |
| 100 | Email delegation agent (autonomous email handling) | 10.3 | XL | pending |

### 3C. Collaboration (if multi-user)

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 101 | Shared inbox | XL | pending |
| 102 | Internal comments (invisible to external) | L | pending |
| 103 | Collision detection ("someone else is replying") | L | pending |
| 104 | Shared drafts | M | pending |
| 105 | Team email digest | M | pending |

### 3D. Integrations

| # | Feature | Effort | Status |
|---|---------|--------|--------|
| 106 | Calendar integration (CalDAV/Google Calendar) | XL | pending |
| 107 | Cloud storage for large attachments | L | pending |
| 108 | Cross-platform context (Slack, Jira, Docs) | XL | pending |
| 109 | Import/export (mbox, EML) | M | pending |
| 110 | Custom domain support | M | pending |

### 3E. Experimental

| # | Feature | Brainstorm # | Effort | Status |
|---|---------|-------------|--------|--------|
| 111 | Predictive inbox (morning forecast) | 10.2 | L | pending |
| 112 | Persistent email memory (never forgets context) | 10.1 | L | pending |
| 113 | Email negotiation assistant | 10.6 | L | pending |
| 114 | Response simulation (predict recipient reaction) | 10.10 | L | pending |
| 115 | Thread fork/merge | 10.12 | M | pending |
| 116 | Contextual snooze (AI picks optimal return time) | 10.13 | S | pending |
| 117 | Email time travel (reconstruct past inbox state) | 10.19 | L | pending |
| 118 | Emotional intelligence coach | 10.9 | M | pending |
| 119 | Email-to-knowledge-base | 10.8 | M | pending |
| 120 | Agent-to-agent protocols | 8.7 | XL | pending |
| 121 | Ambient audio email (voice interaction) | 10.14 | XL | pending |
| 122 | Zero-config AI (progressive personalization from day 1) | 10.20 | L | pending |

**Wave 3 Total: 35 items**

---

## Summary

| Wave | Items | Focus |
|------|-------|-------|
| **Wave 1** — Feature Parity | 36 | Must-have for launch |
| **Wave 2** — Differentiation | 51 | Makes Iris special |
| **Wave 3** — Advanced | 35 | Post-launch ambition |
| **Total** | **122** | |

### Already Built (not on this list)

These shipped in V1–V11 and are done:

- Compose, Reply, Reply-All, Forward (V3)
- Draft auto-save (V3)
- Unified inbox + multi-account (V1, V4)
- Category tabs + AI classification (V4, V6)
- Conversation threading (V2)
- Full-text search with FTS5 (V5)
- Natural language search / AI chat (V8, V11)
- Semantic search via Memories (V10)
- Thread summarization (V7)
- AI assist — rewrite/formal/casual/shorter/longer (V7)
- Agentic tool use — search, list, read, inbox stats (V11)
- Trust indicators — SPF/DKIM/DMARC (V9)
- Tracking pixel detection (V9)
- Agent API + API keys + audit logging (V9)
- Privacy-preserving local AI — Ollama (V6)
- Dark/Light theming with design tokens (UI Overhaul)
- Multi-provider AI — Ollama, Anthropic, OpenAI (Multi-Provider)
- Job queue + background worker (AI Scalability)
- Cross-session chat memory (AI Scalability)
- View mode toggle (V4)
- Batch actions — mark read, archive, delete (V4)
- Session auth with OAuth (Hardening)
- Docker deployment with health checks (Hardening)

### Effort Key

| Size | Time Estimate | Examples |
|------|-------------|---------|
| XS | < 30 min | Config change, CSS fix, one-liner |
| S | 30 min – 2 hours | Small component, simple endpoint |
| M | 2 – 6 hours | New feature with backend + frontend |
| L | 6 – 16 hours | Complex feature, multiple components |
| XL | 16+ hours | Major system, new infrastructure |
