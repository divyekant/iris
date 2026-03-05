# Iris — UI Design & Competitor UX Comparison

- **Date**: 2026-03-01
- **Status**: Research complete

---

## Overview

This document details Iris's UI design for every major screen, then compares it point-by-point with Gmail and Superhuman — the two most relevant competitors (Gmail = market leader, Superhuman = premium UX benchmark).

---

## 1. Layout Architecture

### Iris Layout

```
┌─────────────────────────────────────────────────────────────┐
│  🔵 Iris    [🔍 Search...]              [👤 Account ▾] [⚙]  │
├────────┬──────────────────────────────────────┬─────────────┤
│        │                                      │             │
│  NAV   │         MAIN CONTENT                 │  CHAT (P4)  │
│  BAR   │         (swappable)                  │  (toggle)   │
│        │                                      │             │
│        │                                      │             │
│        │                                      │             │
│        │                                      │             │
│        │                                      │             │
│        │                                      │             │
│        │                                      │             │
└────────┴──────────────────────────────────────┴─────────────┘
```

- **Left nav** (narrow): Folders, special views (Feed), account switcher
- **Main area**: Swaps between Inbox, Thread View, Search, Feed, Settings
- **Right panel**: Chat Panel slides in/out on toggle; not always visible
- **Top bar**: Search, account, settings — persistent

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| Primary layout | Left sidebar + center list + optional right reading pane | Single column, no left sidebar | Left nav + center content + optional right chat panel |
| Navigation model | Mouse + some keyboard | Keyboard-first (vim: J/K/H/L) + command palette (Cmd+K) | Keyboard-first + command palette + mouse |
| Sidebar | Persistent folder/label tree (always visible) | None — no folders, no label tree | Narrow persistent nav (folders + special views) |
| Right panel | Reading pane (optional), add-ons panel | Contact pane, calendar (contextual) | AI Chat Panel (toggled) |
| Command palette | No | Yes — central to UX, teaches shortcuts | Yes — same concept, with AI integration |

### Key Differences

- **Gmail** is mouse-first with keyboard as secondary. Heavy chrome, lots of visual elements competing for attention.
- **Superhuman** strips everything away — no folders, no sidebar. Pure keyboard flow with command palette as the universal entry point. Minimal visual chrome.
- **Iris** takes a middle path: persistent nav for discoverability (unlike Superhuman's hidden navigation), but keyboard-first for speed. The unique element is the **AI Chat Panel** — neither Gmail nor Superhuman has a persistent conversational interface for inbox interaction.

---

## 2. Inbox View

### Iris — Traditional Mode

```
┌────────┬──────────────────────────────────────────────────────┐
│        │  Primary   Updates   Social   Promotions  +Custom    │
│ 📥 All │──────────────────────────────────────────────────────│
│        │  ☐ [⭐] Sarah Chen                          2:34 PM │
│ 📨 Pri │     Re: Q3 Budget Review — Here are the numbers...  │
│ 🔄 Upd │     📎 budget-q3.xlsx            🔴 Urgent · Action │
│ 💬 Soc │                                  ⏰ Due: Friday      │
│ 📢 Pro │──────────────────────────────────────────────────────│
│        │  ☐ [⭐] John + 3 others                     1:15 PM │
│ 📰 Fee │     Falcon Project Standup — Blockers: API rate...   │
│        │     3 messages                    🟡 Needs Reply     │
│ ─────  │──────────────────────────────────────────────────────│
│ 📁 Lab │  ☐ [☆] GitHub                             12:42 PM │
│ 📁 Arc │     [dependabot] Bump axios from 1.6 to 1.7         │
│ 📁 Sen │                                   ⚪ Informational   │
│ 📁 Dra │──────────────────────────────────────────────────────│
│ 🗑 Tra │                                                      │
│        │  [Traditional ◉ ○ Messaging]     Showing 1-50 of 234│
└────────┴──────────────────────────────────────────────────────┘
```

### Iris — Messaging Mode

```
┌────────┬──────────────────────────────────────────────────────┐
│        │  Primary   Updates   Social   Promotions  +Custom    │
│ 📥 All │──────────────────────────────────────────────────────│
│        │                                                      │
│ 📨 Pri │  ┌──────────────────────────────────┐      2:34 PM  │
│ 🔄 Upd │  │ SC  Sarah Chen           🔴 Urgent│               │
│ 💬 Soc │  │     Here are the numbers you      │               │
│ 📢 Pro │  │     asked for. See attached.       │               │
│        │  │     📎 budget-q3.xlsx              │               │
│ 📰 Fee │  │     ⏰ Due: Friday                 │               │
│        │  └──────────────────────────────────┘               │
│ ─────  │                                                      │
│ 📁 Lab │  ┌──────────────────────────────────┐      1:15 PM  │
│ 📁 Arc │  │ JD  John + 3         🟡 Needs Reply│              │
│ 📁 Sen │  │     Blockers: API rate limiting    │               │
│ 📁 Dra │  │     is holding up the deploy...    │               │
│ 🗑 Tra │  │                         3 msgs    │               │
│        │  └──────────────────────────────────┘               │
│        │                                                      │
│ ⚙ Set  │  [Traditional ○ ◉ Messaging]     Showing 1-50 of 234│
└────────┴──────────────────────────────────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| View modes | 1 (list only) | 1 (single column list) | 2 (Traditional list + Messaging cards) |
| Categorization tabs | 5 fixed (Primary, Social, Promotions, Updates, Forums) | Split Inbox (2-7 custom splits, AI + manual rules) | Dynamic tabs (AI-assigned + user-created, evolve over time) |
| Message row info | Sender, subject snippet, time | Sender, subject snippet, time, 1-line AI summary below subject | Sender, subject, snippet, time, priority badge, intent label, deadline |
| Priority indication | None visible (internal sorting only) | Auto Labels appear as split filters, not inline badges | Colored priority dots inline: 🔴 Urgent, 🟡 Needs Reply, ⚪ FYI |
| Intent classification | None | Auto Labels: "Response Needed," "Waiting On," "Meetings," etc. (used as split filters) | Inline labels: "Action," "Needs Reply," "Informational," "FYI," "Sales" |
| Deadline visibility | None | None inline | ⏰ Extracted deadlines shown on message row |
| Unread styling | Bold text + dot | Blue left accent bar | Bold text + accent bar (configurable) |

### Key Differences

- **Gmail** shows raw data — you mentally sort what's important. No intelligence visible in the list.
- **Superhuman** separates email into splits (tabs) so you triage by category. Auto Labels sort FOR you, but the information lives in the split structure, not on individual rows. Each row is still just sender + subject + time.
- **Iris** puts AI metadata **on every row** — you don't need to organize emails into splits to see priority. Every message tells you: how urgent is this, what does it want from me, when is it due. Plus the messaging mode toggle that neither competitor has.

**Superhuman's Split Inbox vs Iris's Dynamic Categories:**

Superhuman's splits are powerful but user-configured (even with AI labels, you build the splits). Iris's categories are AI-assigned from day one and evolve — new categories emerge organically as the AI learns your patterns (e.g., "Client Escalations" appears after it detects frustrated tone from known clients repeatedly).

---

## 3. Thread View

### Iris Thread View

```
┌────────┬──────────────────────────────────────────────────────┐
│        │  ← Back                   [Archive] [Label] [Snooze] │
│  NAV   │──────────────────────────────────────────────────────│
│        │  Re: Q3 Budget Review                                │
│        │  Sarah Chen, You, Finance Team         3 messages    │
│        │                                                      │
│        │  ┌─ AI Summary ──────────────────────────────── ▾ ─┐ │
│        │  │ Sarah shared Q3 budget numbers. Total is $1.2M, │ │
│        │  │ 8% over target. She's asking you to review the  │ │
│        │  │ marketing line items by Friday.                  │ │
│        │  │ ⏰ Deadline: Friday · 🎯 Action: Review budget  │ │
│        │  └─────────────────────────────────────────────────┘ │
│        │                                                      │
│        │  Sarah Chen                    2:34 PM  🟢 Verified  │
│        │  ┌─────────────────────────────────────────────────┐ │
│        │  │ Hi, Here are the Q3 numbers. We're slightly     │ │
│        │  │ over on marketing — can you review lines 14-22? │ │
│        │  │ 📎 budget-q3.xlsx (24 KB)                       │ │
│        │  └─────────────────────────────────────────────────┘ │
│        │                                                      │
│        │  You                           1:15 PM               │
│        │  ┌─────────────────────────────────────────────────┐ │
│        │  │ Thanks Sarah, I'll take a look today.           │ │
│        │  └─────────────────────────────────────────────────┘ │
│        │                                                      │
│        │  ┌─────────────────────────────────────────────────┐ │
│        │  │ 💬 Reply...                    [Reply All] [Fwd] │ │
│        │  └─────────────────────────────────────────────────┘ │
└────────┴──────────────────────────────────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| AI summary | Gemini summary (opt-in, at top) | 1-line auto-summary below subject, press M to expand | Multi-line summary with extracted deadline + action items, collapsible |
| Summary content | Generic summary of thread | Summary of conversation | Summary + **extracted deadline** + **extracted action item** |
| Trust indicators | Hidden in "Show original" | None visible | 🟢 Verified / 🟡 Partial / 🔴 Failed badge on every message (SPF/DKIM/DMARC) |
| Tracking detection | None | None (Superhuman itself uses tracking pixels) | Yellow alert bar when tracking pixels detected |
| Quick reply | Reply box at bottom | Instant Reply: 3 AI-generated full replies (Tab to cycle, Enter to insert) | Reply bar at bottom + AI assist available |
| Thread collapse | Older messages collapsed | Older messages collapsed, AI summary provides context | Older messages collapsed, AI summary provides context |
| Contact info | Basic hover card | Rich Contact Pane sidebar (LinkedIn, company, recent threads) | Contact info in sidebar (future: AI-generated relationship profile) |

### Key Differences

- **Gmail's** Gemini summary is a plain text summary — useful but doesn't extract structured data.
- **Superhuman's** 1-line summary is elegant and fast (always visible, not opt-in), with 3 AI reply options that let you respond in seconds without typing.
- **Iris** adds **structured extraction**: deadline dates and action items pulled out as discrete, actionable metadata — not just prose summary. Plus trust badges and tracking detection that protect the user (Superhuman notably uses tracking pixels itself).

**Superhuman's Instant Reply is a strong UX innovation** — 3 full-email AI replies you cycle through with Tab. Iris should adopt this pattern. Currently the Iris breadboard has AI assist in compose but not pre-generated reply options in the thread view. This is a gap.

---

## 4. Compose

### Iris Compose

```
┌─────────────────────────────────────────────────────────────┐
│  ✕  New Message                              [⋯] [Minimize] │
├─────────────────────────────────────────────────────────────┤
│  To:   [sarah@company.com              ▾ AI suggests: +CFO] │
│  Cc:   [                                                   ] │
│  Subj: [Q3 Budget Feedback                                 ] │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  Hi Sarah,                                                   │
│                                                              │
│  I reviewed the Q3 numbers. Marketing lines 14-22 look      │
│  reasonable given the new campaign launch, but I'd like      │
│  to discuss line 18 (agency fees) — seems 40% above Q2.     │
│                                                              │
│  Can we chat Thursday?                                       │
│                                                              │
│  Best,                                                       │
│  D                                                           │
│                                                              │
├─────────────────────────────────────────────────────────────┤
│  [🤖 AI: Rewrite ▾] [Formal/Casual] [Translate]   Markdown ◉│
│  📎 Attach    [Signature ▾]      [Schedule ▾] [Send]        │
│                              Undo window: 5-30s configurable │
└─────────────────────────────────────────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| Open compose | Small window at bottom-right corner | Modal overlay (C key, ~2s load) | Modal overlay (C key) |
| AI writing | "Help Me Write" — prompt-based, generates draft | Write with AI (Cmd+J) — prompt-based, learns your voice per recipient | AI Assist — rewrite, tone toggle, translate. Learns voice over time |
| Smart CC | None | None | AI suggests who should be CC'd based on thread/topic patterns |
| Templates/Snippets | Templates in settings (hidden) | Snippets: ;trigger or Cmd+; with variables ({first_name}, custom) | Templates with variables + AI-generated templates from repeated patterns |
| Tone control | None | "Rewrite in your voice" adapts per recipient | Formal/Casual toggle, tone slider |
| Markdown | No | Yes (native) | Yes (native, with toggle) |
| Scheduling | Yes (pick date/time) | Yes + timezone-aware optimization (claims 20-30% better open rates) | Yes (future: AI-suggested optimal send times) |
| Undo send | 5-30s configurable | 10s, Z to undo | 5-30s configurable, Z to undo |
| Auto-generated replies | Smart Reply (3 short phrases) | Instant Reply (3 full emails in your voice, Tab to cycle) | AI-generated reply options (planned — learning from Superhuman's pattern) |

### Key Differences

- **Gmail's** compose is adequate but uninspired. "Help Me Write" is prompt-based and doesn't learn your style.
- **Superhuman's** compose has two standout features: (1) Snippets with `;trigger` inline insertion are genuinely faster than any template system, and (2) Voice learning per recipient — it writes differently to your boss vs your friend.
- **Iris** adds Smart CC suggestions (nobody does this) and the formal/casual toggle. The translate feature is useful for international users. But Iris should adopt Superhuman's `;trigger` snippet pattern — it's the fastest template insertion model that exists.

---

## 5. Search

### Iris Search

```
┌────────┬──────────────────────────────────────────────────────┐
│        │  🔍 [what did sarah say about the budget?        ]   │
│  NAV   │  [Date ▾] [From ▾] [Has: 📎] [Label ▾] [🧠 Semantic]│
│        │──────────────────────────────────────────────────────│
│        │                                                      │
│        │  ┌─ Answer ────────────────────────────────────────┐ │
│        │  │ Sarah said the Q3 budget is $1.2M, 8% over     │ │
│        │  │ target. She asked you to review marketing line  │ │
│        │  │ items 14-22 by Friday.                          │ │
│        │  │ 📧 Source: "Re: Q3 Budget Review" — Mar 1       │ │
│        │  └─────────────────────────────────────────────────┘ │
│        │                                                      │
│        │  3 results                                           │
│        │──────────────────────────────────────────────────────│
│        │  Sarah Chen — Re: Q3 Budget Review        Mar 1     │
│        │  "...we're slightly over on **marketing**..."       │
│        │──────────────────────────────────────────────────────│
│        │  Sarah Chen — Budget Planning Q3          Feb 15    │
│        │──────────────────────────────────────────────────────│
│        │  You → Sarah — Q3 Targets                 Feb 10    │
│        │──────────────────────────────────────────────────────│
└────────┴──────────────────────────────────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| Search speed | Fast (server-side, Google-scale) | Fastest (local database, instant) | Fast (local FTS5 + server fallback) |
| Natural language | Gemini Q&A (limited, single-email scope) | Ask AI: NL questions with answers citing emails (Business plan only, $40/mo) | NL search with answer extraction (included, runs on local AI) |
| Semantic search | No | Ask AI has semantic understanding | Yes — embedding-based similarity search, toggleable |
| Answer extraction | No (returns list of emails) | Yes (Ask AI returns answers with email citations) | Yes — answer panel above results with source citation |
| Search operators | Most complete (from:, to:, has:, date:, size:, AND/OR/NOT) | Supports Gmail's operator set | Full operator set + natural language alternative |
| Attachment content search | Yes (PDFs, Docs, spreadsheets — server-side) | No | Future (local parsing + indexing) |
| Filter chips | Hidden under dropdown | Available | Visible chips (Date, From, Has:attachment, Label, Unread) |
| Saved searches | No | No | Yes (Smart Folders) |
| Cross-thread answers | No | Yes (Ask AI spans threads) | Yes (RAG over entire email corpus) |

### Key Differences

- **Gmail** has the best raw keyword search (decades of optimization, attachment content search). But no semantic understanding and no answer extraction.
- **Superhuman's Ask AI** is the closest competitor to Iris's search — it answers questions with citations across threads. But it's a $40/mo add-on and runs on external AI (not local/private).
- **Iris** matches Superhuman's Ask AI capabilities but runs everything **locally** — your search queries and email content never leave your machine. Plus the explicit semantic toggle lets power users choose between precision (keyword) and recall (semantic).

---

## 6. AI Interaction Model

### Iris Chat Panel

```
┌─────────────────────────┐
│  💬 Iris AI        [✕]  │
│─────────────────────────│
│                         │
│  ┌─ You ─────────────┐  │
│  │ What am I supposed │  │
│  │ to do today?       │  │
│  └────────────────────┘  │
│                         │
│  ┌─ Iris ─────────────┐  │
│  │ Based on today's    │  │
│  │ emails, you have:   │  │
│  │                     │  │
│  │ 🔴 Urgent (2):      │  │
│  │ • Review Q3 budget  │  │
│  │   📧 from Sarah     │  │
│  │ • Approve deploy    │  │
│  │   📧 from DevOps    │  │
│  │                     │  │
│  │ 🟡 Action (3):      │  │
│  │ • Confirm meeting   │  │
│  │ • Send Acme update  │  │
│  │ • Review PR #847    │  │
│  │                     │  │
│  │ 📊 5 FYI, 12 notifs │  │
│  └────────────────────┘  │
│                         │
│  [Briefing] [Tasks]     │
│  [Search]  [Compose]    │
│                         │
│  ┌─────────────────────┐│
│  │ Ask anything...    ⏎││
│  └─────────────────────┘│
└─────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| AI interaction model | Gemini: per-email assistance (summarize this, help me write) | In-app: per-email (Instant Reply, Write with AI, Auto Summary). **Superhuman Go**: separate agentic assistant in browser sidebar (cross-app, 100+ agents) | Chat Panel: persistent conversational AI with full inbox RAG context |
| Scope of AI context | Single email or thread | Single email/thread (in-app). Full inbox + calendar + other apps (Go) | Entire email corpus — all threads, all time |
| Bulk operations | None via AI | None via AI (manual bulk actions exist) | "Archive all LinkedIn older than a month" via NL command |
| Briefings | None | None (though Auto Labels provide similar triage) | "Give me a briefing" — structured summary of inbox state |
| Task extraction | Gemini can summarize action items from one email | None explicit | "What do I need to do today?" — aggregated from all emails |
| Compose via chat | No | No (compose is separate, AI assists within it) | "Tell Sarah the project is delayed, apologetic tone" → draft created |
| Where AI lives | Inside email view (inline, per-message) | Inside email view (inline) + separate browser sidebar (Go) | Persistent sidebar panel, always available from any screen |
| Privacy | Cloud (Google processes all emails) | Cloud (Superhuman processes all emails) | Local (Ollama on your machine, email never sent externally) |

### Key Differences

- **Gmail's Gemini** is trapped inside individual emails. It can't see across threads, can't do bulk operations, can't give you a briefing of your day.
- **Superhuman** has the best per-email AI (Instant Reply is genuinely brilliant), plus Superhuman Go as a separate browser-wide agent. But Go is a different product, not integrated into the email client.
- **Iris** unifies the conversational AI into the email client itself — one chat panel that can do everything: search, brief, compose, bulk actions, answer questions. With RAG over your entire email history, not just the current thread. And it all runs locally.

---

## 7. Newsletter / Feed Experience

### Iris Newsletter Feed

```
┌────────┬──────────────────────────────────────────────────────┐
│        │  📰 Feed            [Manage Subscriptions]           │
│  NAV   │──────────────────────────────────────────────────────│
│        │                                                      │
│        │  ┌─────────────────────────────────────────────────┐ │
│        │  │  TLDR Newsletter                    Today 6:00AM│ │
│        │  │─────────────────────────────────────────────────│ │
│        │  │  🤖 AI chip exports restricted to 5 more        │ │
│        │  │  countries. NVIDIA shares drop 3%...             │ │
│        │  │  📱 Apple announces M5 Ultra with 128GB         │ │
│        │  │  unified memory...                              │ │
│        │  │  [Read full] [Unsubscribe]                      │ │
│        │  └─────────────────────────────────────────────────┘ │
│        │                                                      │
│        │  ┌─────────────────────────────────────────────────┐ │
│        │  │  Hacker Newsletter              Yesterday 8:00AM│ │
│        │  │─────────────────────────────────────────────────│ │
│        │  │  Top stories: SQLite as a document database...  │ │
│        │  │  [Read full] [Unsubscribe]                      │ │
│        │  └─────────────────────────────────────────────────┘ │
└────────┴──────────────────────────────────────────────────────┘
```

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| Newsletter handling | Buried in Promotions tab, mixed with spam + ads | Split into "News/Newsletters" split (separate tab, but still shows as email rows) | Dedicated Feed view with **inline content rendering** (like RSS reader) |
| Content rendering | Must open each email to read | Must open each email to read | Content rendered **inline** in the feed — scroll to read without opening |
| Subscription management | None built-in (rely on Unroll.me etc.) | None built-in | Dashboard: sources, open rates, one-click unsubscribe per source |
| Newsletter discovery | Mixed into inbox noise | Separated by split, but still competes with other email | Completely separated — own nav item, own view, own experience |

### Key Differences

This is an area where **Iris is genuinely better than both competitors**:

- **Gmail** treats newsletters as second-class citizens. They go to Promotions and die unread.
- **Superhuman** gives newsletters a split tab, which is better, but they still render as email rows you have to open individually.
- **Iris** renders newsletter content inline like an RSS reader. You scroll through your subscriptions like reading a feed — no click-to-open, no email chrome. Plus subscription management with open rate tracking and one-click unsubscribe.

HEY's "The Feed" proved this UX works. Iris builds on it.

---

## 8. Speed & Interaction

### Comparison

| Aspect | Gmail | Superhuman | Iris (Target) |
|--------|-------|-----------|---------------|
| Interaction speed target | No public target | 100ms public / 50-60ms internal | 100ms target |
| Optimistic UI | Some (archive is instant) | Everything (all actions instant, sync in background) | Yes — same pattern as Superhuman |
| Data locality | Server-side (round-trip for everything) | Local database + prefetching | Local SQLite (all data on disk, instant reads) |
| Keyboard shortcuts | ~50, not customizable | 100+, vim-style (J/K/H/L), customizable | 100+, vim-style, fully customizable |
| Command palette | No | Yes (Cmd+K) — central UX, teaches shortcuts | Yes (Cmd+K) — with AI integration |
| Undo model | Toast notification (5-30s) | Toast + Z key (10s for send) | Toast + Z key (5-30s configurable) |
| Onboarding for speed | None | 30-min 1:1 with synthetic data to build muscle memory | Interactive tutorial with progressive shortcut introduction |
| Key repeat rate | Browser default | 65ms (faster than OS default) | Will tune similarly |
| Removal animations | Standard browser | 150ms slide-out (tuned for "snappy but not jarring") | 150ms target (learn from Superhuman's tuning) |
| Prefetching | Minimal | Adjacent emails preloaded before navigation | Adjacent emails preloaded |

### Key Differences

- **Gmail** is fast because Google's servers are fast. But it's not *optimized for perceived speed* — there are loading spinners, round-trips, and no undo-as-safety-net pattern.
- **Superhuman** is the gold standard for perceived speed. Every interaction is carefully tuned: optimistic UI, local data, prefetching, animation timing, keyboard shortcuts. This is their moat.
- **Iris** can match Superhuman's speed patterns because it's also local-data-first. The local SQLite database gives us the same instant-display advantage. We should explicitly adopt their patterns: optimistic UI, 150ms animations, prefetching, tuned key repeat.

**Honest gap**: Superhuman has years of A/B testing and micro-optimization on speed perception. Iris will need to invest significantly in polish to match the *feel*, even if the raw speed is comparable.

---

## 9. Privacy & Trust

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| Who processes your email | Google (cloud) | Superhuman (cloud) | You (local machine only) |
| AI data handling | Gemini processes on Google servers | AI features process on Superhuman servers | Ollama runs locally — nothing leaves your machine |
| Tracking pixels | Proxies images (hides IP but confirms open) | **Uses tracking pixels** for read receipts — controversial | **Detects and blocks** tracking pixels, alerts user |
| Read receipts | No | Yes (times opened, device, approximate location) | No (privacy-first stance, like HEY) |
| Email authentication | Hidden in "Show original" headers | Not surfaced | Visible trust badge on every message (🟢🟡🔴) |
| Privacy report | No | No | Tracking pixel report per sender (like HEY's Spy Pixel report) |
| Telemetry | Extensive (Google analytics) | Some (usage analytics) | None — zero telemetry, zero tracking |
| Data location | Google's servers worldwide | Superhuman's servers | Your machine / your Docker container |

### Key Differences

This is Iris's strongest differentiator:

- **Gmail** and **Superhuman** are both cloud services. Your email is processed on their servers. Their AI features require sending your email content through their systems.
- **Superhuman** is particularly notable because it **uses tracking pixels** — the same technology it could warn you about. It tracks when recipients open your emails, how many times, what device, and approximate location. This is a privacy stance that directly conflicts with user privacy advocacy.
- **Iris** takes the opposite position: **detect and block tracking, never track others, process everything locally**. The trust badge (SPF/DKIM/DMARC visualization) makes email authentication accessible to non-technical users. The tracking pixel report tells you who's tracking you.

---

## 10. Agent / API Connectivity

### Comparison

| Aspect | Gmail | Superhuman | Iris |
|--------|-------|-----------|------|
| API access | Gmail API (REST, Google-controlled) | No public API | REST/GraphQL API + MCP server (user-controlled) |
| External agent access | Via Gmail API + Google OAuth | Superhuman Go (browser sidebar, proprietary agent network) | Any MCP-compatible agent connects directly |
| Permission model | Google OAuth scopes (coarse: read, send, modify) | Go agents have predefined capabilities | Granular: read-only, draft-only, send-with-approval, autonomous, per-label scoping |
| Rate limits | Google-imposed (15 IMAP connections, bandwidth limits) | N/A (no API) | None (it's your machine) |
| Webhooks | Gmail Pub/Sub (server-to-server, complex setup) | No | User-configured webhooks on email events |
| Audit trail | Google Workspace audit log (admin only) | No | Full audit log of every agent action |
| Agent marketplace | Google Workspace Add-ons marketplace | Superhuman Go Agents SDK (private beta, curated partners) | Open — any agent, any developer, via standard MCP/REST |

### Key Differences

- **Gmail** has an API but it's Google's. Rate limits, OAuth complexity, and potential for API changes or deprecation (they killed POP in Q1 2026).
- **Superhuman Go** is an agentic platform but it's proprietary and curated. You use their agents (Jira, Box, etc.) through their SDK. It's powerful but closed.
- **Iris** is the **open platform**. MCP server means any Claude instance, any AI agent, any developer tool can interact with your email. Permission framework means you control exactly what each agent can do. No rate limits because it's your machine. This is the true differentiator — nobody else has a locally-controlled, open-standard, granular-permission agent connectivity layer.

---

## 11. Features Iris Should Adopt from Competitors

Based on this analysis, these competitor features are strong enough that Iris should adopt them:

### From Superhuman

| Feature | Why | Priority |
|---------|-----|----------|
| **Instant Reply** (3 pre-generated full replies in thread view) | Dramatically faster than opening compose. Tab to cycle is brilliant. | High |
| **`;trigger` snippets** with inline insertion | Fastest template insertion pattern that exists | High |
| **Optimistic UI everywhere** | Fundamental to speed perception | High |
| **150ms removal animations** | Tuned value that feels right | Medium |
| **65ms key repeat** | Small but noticeable speed improvement | Medium |
| **Contact Pane** (rich sidebar with social/company data) | Useful context — but Iris's version should be AI-generated from email history, not dependent on Clearbit | Medium |
| **Split Inbox Library** (pre-built split configurations) | Good onboarding — users start with proven configurations | Low |

### From Gmail

| Feature | Why | Priority |
|---------|-----|----------|
| **Attachment content search** | Gmail is one of only 2 clients that searches inside PDFs/docs — powerful feature | Medium |
| **Dense layout option** | Power users want maximum information density | Medium |

### From HEY

| Feature | Why | Priority |
|---------|-----|----------|
| **Screener** (approve/reject unknown senders) | Eliminates most spam at a fundamental level | Medium |
| **Spy Pixel report** | Makes tracking visible and actionable | High (aligns with privacy stance) |

---

## 12. Summary: What Makes Iris's UI Genuinely Different

| # | Differentiator | Gmail | Superhuman | Iris |
|---|---------------|-------|-----------|------|
| 1 | **AI metadata on every row** (priority, intent, deadline) | No | Partial (Auto Labels filter, not inline) | Yes — visible on every message |
| 2 | **Messaging mode toggle** | No | No | Yes — opt-in card layout |
| 3 | **Persistent AI Chat Panel** with full-corpus RAG | No | Separate product (Go) | Built into the email client |
| 4 | **Answer extraction in search** (local/private) | No | Yes (Ask AI, cloud, $40/mo) | Yes (local, included) |
| 5 | **Newsletter feed with inline rendering** | No | No (split tab, still email rows) | Yes — RSS-like reading experience |
| 6 | **Trust badges** (SPF/DKIM/DMARC visible) | No | No | Yes — every message |
| 7 | **Tracking pixel detection + blocking** | Partial | No (uses tracking itself) | Yes — detect, block, report |
| 8 | **Open agent connectivity** (MCP + REST, user-controlled) | API (Google-controlled) | Proprietary (Go SDK) | Open standard, your machine |
| 9 | **Dynamic AI categories** that evolve without config | 5 fixed tabs | User-built splits (powerful but manual) | AI creates and evolves categories from behavior |
| 10 | **Smart CC suggestions** | No | No | Yes — AI suggests missing recipients |
| 11 | **Everything runs locally** | Cloud | Cloud | Local — privacy is architecture |

### The Pitch in One Line

> Gmail shows you email. Superhuman makes you fast at email. Iris understands your email.
