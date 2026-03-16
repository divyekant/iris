# Wave 3 Design Spec — Foundation + Intelligence + Agent Connectivity

**Date:** 2026-03-15
**Status:** Approved
**Approach:** Foundation-first — fix UX before adding features

---

## Context

Wave 1 shipped 36 features (feature parity). Wave 2 shipped 51 features (differentiation). Both prioritized velocity over UX consistency, creating a patchwork experience — 60 Svelte components built by parallel agents with no unifying design pass.

Wave 3 inverts the priority: **polish first, then showcase**.

## Strategy

Three layers, built in order:

1. **Layer 1: UX Foundation** — cohesion pass + interaction quality
2. **Layer 2: Agent Infrastructure** — Rust CLI + deeper MCP server
3. **Layer 3: Showcase Features** — 6 features that demonstrate intelligence

## Inventory

- 87 features built (W1: 36, W2: 51)
- 58 features open (35 on Wave 3 master list + 23 unplanned brainstorm items)
- Wave 3 selects 6 showcase features from themes B (deep intelligence) and C (automation)
- Source lists: `docs/research/04-capability-audit.md`, `docs/research/05-ai-features-brainstorm.md`

---

## Layer 1: UX Foundation

Addresses six confirmed pain points:
1. Lifeless transitions — no animation
2. Information overload — too many badges/buttons
3. Navigation flow — too many clicks, not keyboard-first
4. Visual monotony — everything same weight/importance
5. AI feels bolted on — chat panel separate from core
6. No satisfying feedback loops — actions complete silently

### 1.1 Animation & Transition System

A shared Svelte transition library. Every component uses named transitions from one place.

**Core primitives:**

| Name | Effect | Duration | Use Cases |
|------|--------|----------|-----------|
| `fade` | opacity 0→1 | 120ms | tooltips, popovers, hover actions |
| `slide` | translateY + fade | 200ms | panels, modals, list items |
| `scale` | scale 0.95→1 + fade | 150ms | action confirmations, dialogs |
| `collapse` | height auto-animate | 200ms | expandable sections, row removal |

**Application map:**

| Component | Open/Enter | Close/Exit |
|-----------|-----------|------------|
| Modal | `scale` in | `fade` out |
| Chat panel | `slide` from right | `slide` to right |
| Message row archive/delete | `slide` + `collapse` | — |
| Toast notifications | `slide` up from bottom | `fade` out |
| Settings tab switch | `fade` crossfade | — |
| Compose modal | `slide` up from bottom | `slide` down |
| Hover action buttons | `fade` in (staggered 30ms) | `fade` out |

**Implementation:** `web/src/lib/transitions.ts` — exports named Svelte transitions using CSS custom property values (`--iris-transition-fast: 120ms ease`, `--iris-transition-normal: 200ms ease`). These tokens include both duration and easing. The library uses them directly as CSS `transition` shorthand values. All components import from this single source.

### 1.2 Feedback Loops

Every user action gets acknowledgment.

| Action | Feedback | Mechanism |
|--------|----------|-----------|
| Archive/Delete/Star | Row animates out + toast with undo | `feedback.success("Archived", { undo })` |
| Send email | Undo countdown + success toast with checkmark | Existing undo-send + new completion toast |
| Bulk actions | Progress indicator + completion toast | "Archived 12 conversations" |
| Save settings | Inline "Saved" flash next to button | Contextual, not toast — keeps focus |
| AI processing | Subtle pulse on element, result slides in | CSS animation + `slide` transition |
| Keyboard action | Brief gold flash on affected row border | 200ms highlight animation |

**Implementation:** `web/src/lib/feedback.ts` — a Svelte store. Components call `feedback.success(msg, opts)`, `feedback.undo(msg, undoFn)`. Toast component subscribes and renders with animations.

### 1.3 Visual Hierarchy & Information Density

**Message Row redesign (Smart Badge Priority):**

- Default visible: sender, subject (bold if unread), snippet, relative date, unread gold bar
- ONE primary badge slot, algorithmic priority: `needs_reply` > `deadline` > `intent`
- Secondary signals (sentiment, labels, category) on hover as mini-popover
- Max 2 badges visible; overflow shows "+N" chip

**Visual weight system:**

| Level | Token | Use |
|-------|-------|-----|
| Primary | `--iris-color-text` / font-weight 500 | Sender names, subjects, headings |
| Secondary | `--iris-color-text-muted` / font-weight 400 | Snippets, metadata, timestamps |
| Tertiary | `--iris-color-text-faint` / font-weight 400 | Counts, minor labels |
| Active | `color-mix(primary 8%, transparent)` | Selected/focused rows |
| Unread | Bold sender + gold left border | Keep existing pattern |

**ThreadView action bar (Grouped):**

| Group | Actions | Presentation |
|-------|---------|-------------|
| Primary (always visible) | Reply, Reply All, Forward | Icon buttons |
| Organize (dropdown) | Star, Snooze, Archive, Delete | Dropdown menu |
| AI (dropdown) | Summarize, Tasks, Multi-Reply | Dropdown menu |
| More (dropdown) | Spam, Mute, Redirect | Dropdown menu |

Dividers between groups. Keyboard shortcuts still work directly (no dropdown needed).

### 1.4 Keyboard-First Navigation

**Command Palette (`Cmd+K`):**

Searchable palette for everything — not just search. Type actions, navigate, configure.

Examples:
- "archive" → archive focused message
- "settings ai" → jump to AI settings tab
- "compose to sarah" → open compose pre-filled
- "chat summarize today" → open chat with query
- "theme dark" → switch theme

**Extended keyboard navigation:**

| Context | Keys | Action |
|---------|------|--------|
| Global | `Cmd+K` | Command palette |
| Global | `c` | New compose |
| Global | `/` | Focus search |
| Inbox | `j/k` | Navigate rows (existing) |
| Inbox | `e` | Archive focused |
| Inbox | `s` | Star focused |
| Inbox | `#` | Delete focused |
| Inbox | `r` | Reply to focused |
| Inbox | `b` | Snooze focused (new) |
| Inbox | `m` | Mute focused (new) |
| Settings | `h/l` | Switch tabs |
| Thread | `j/k` | Jump between messages |
| Chat | `j/k` | Scroll messages |
| Any | `?` | Keyboard help overlay |

**Mode indicators:** Subtle badge in bottom-left showing current mode (Inbox / Thread / Compose / Chat). Keyboard users always know context.

**Focus management:** Visible gold focus ring matching brand. No invisible focus states.

### 1.5 AI Integration (Woven, Not Bolted)

AI surfaces throughout the app, not just in the chat panel.

**Inline AI suggestions:**
- Thread that needs reply → subtle "AI suggests..." strip below thread header
- 1-2 sentence draft preview, click to expand into compose
- Appears only for `needs_reply` flagged threads

**Contextual AI actions:**
- Right-click message row → "Summarize", "Extract tasks", "Draft reply"
- Results appear inline (not in chat panel)
- Uses existing API endpoints, new UI surface

**Smart compose hints:**
- Dropdown-style autocomplete suggestions (extending existing `AutocompleteTextarea.svelte`)
- Suggestions appear below cursor as a floating menu, triggered by pause in typing (500ms debounce)
- Arrow keys to select, Tab/Enter to accept, Escape to dismiss
- Uses existing autocomplete endpoint with thread context injection
- Note: Copilot-style inline ghost text was considered but scoped out — requires custom editor overlay that conflicts with both textarea and TipTap. The dropdown pattern works with both editors and is already proven in the codebase.

**Thread intelligence strip:**
- Replaces separate summary panel
- Single line below thread subject: "5 messages, 2 action items, deadline Mar 20"
- Click to expand full summary
- Always visible for multi-message threads

**Chat panel refinement:**
- Stays as deep conversation interface
- Becomes contextual: opening while viewing a thread pre-loads that thread as context
- "What did John mean by..." works without specifying which thread

### 1.6 Shared Component Library

| Component | Status | Changes |
|-----------|--------|---------|
| `Modal.svelte` | **New** | Shared modal: backdrop, escape handler, size variants (sm/md/lg), `scale` transition. Refactor all existing dialogs (SpamDialog, RedirectDialog, SnoozePicker, etc.) to use it. |
| `ModalActions.svelte` | **New** | Consistent button row (cancel left, primary right) for all modals |
| `FormInput.svelte` | **New** | Styled text input with label, placeholder, error state, focus ring |
| `FormSelect.svelte` | **New** | Styled select with consistent border/focus |
| `FormToggle.svelte` | **New** | iOS-style toggle replacing manual toggle implementations |
| `Badge.svelte` | **New** | Semantic color variants (success/warning/error/info/neutral), size variants (sm/md) |
| `Toast.svelte` | Exists | Wire to feedback store; add undo support; add animations |
| `DropdownMenu.svelte` | **New** | Replace ad-hoc dropdowns in ThreadView, TopNav |
| `CommandPalette.svelte` | **New** | `Cmd+K` palette |
| `Tooltip.svelte` | **New** | Replace title attributes with styled, animated tooltips |

Note: Modal, ModalActions, Form*, and Badge were identified as needed in the UI cohesion shaping doc but never implemented. They are prerequisites for 1.5 (AI woven), 1.7 (Settings refactor), and all Layer 3 features.

### 1.7 Settings Refactor

Settings is already split into 6 tab components (from Wave 2). Remaining work:

- Add URL hash routing (`#settings/ai`) for deep linking and back/forward
- Consistent section headers, form layouts, save feedback across all tabs
- Each tab lazy-loads on click (not all rendered eagerly)
- Verify all tabs use shared FormInput/FormSelect/FormToggle components

### 1.8 Token Compliance

Re-audit needed before implementation — prior references to `operatorColors` in Search.svelte and `pillColors` in ContactTopicsPanel.svelte may be stale (possibly already fixed or renamed). Run `grep -r '#[0-9a-fA-F]\{3,8\}' web/src/ --include='*.svelte'` to find actual remaining hardcoded hex values and fix all instances.

---

## Layer 2: Agent Infrastructure

### 2.1 Rust CLI (`iris` binary)

Subcommand-based CLI in the same Cargo workspace. Talks to running Iris server over HTTP.

**Subcommands:**

```
iris inbox                       # unread count, top 10 messages
iris inbox --all --limit 50      # full list with pagination
iris read <thread-id>            # display thread messages
iris search "budget Q3"          # full-text search
iris search --semantic "..."     # semantic search via Memories
iris send --to ... --subject ... --body ...
iris send --reply-to <msg-id> --body ...
iris draft create --to ... --subject ... --body ...
iris draft list
iris chat "summarize unread"     # agentic chat
iris chat --session <id> "..."   # continue conversation
iris ai classify <msg-id>       # trigger AI classification
iris ai summarize <thread-id>   # thread summary
iris config get <key>            # read config
iris config set <key> <value>    # write config
iris status                      # health, sync status, queue depth
iris keys list                   # API key management
iris keys create --permission read_only --name "..."
```

**Auth:** Config file at `~/.iris/config.toml` with server URL + API key. `iris init` for first-time setup.

**Output:**
- Default: human-readable, colored terminal output
- `--json`: structured JSON for programmatic use
- `--quiet`: minimal output (IDs only) for scripting

**Dependencies:** `clap` (arg parsing), `reqwest` (HTTP), `serde_json` (serialization). Shares types with server via `iris-common` crate.

### 2.2 Deeper MCP Server

Expand existing MCP server with richer tools, resources, and prompts.

**Current state:** The existing `src/api/mcp_server.rs` provides session management, tool schemas, and tool call execution. Existing tools include: `search_emails`, `read_message`, `compose_draft`, `send_email`, `label_message`, `archive_message`. The following are net-new additions:

**New tools (added to existing):**

| Tool | Description |
|------|-------------|
| `list_threads` | Filter by unread, starred, category, date range, sender |
| `get_thread_summary` | AI-generated thread summary |
| `get_contact_profile` | Relationship intelligence for a contact |
| `extract_tasks` | Action items from a thread |
| `extract_deadlines` | Deadlines from a thread |
| `chat` | Stateful conversational interaction with citations |
| `get_inbox_stats` | Unread count, category breakdown, needs-reply count |
| `manage_draft` | Create/update/delete drafts |
| `bulk_action` | Archive/delete/star/mark-read on multiple messages |

**Resources (MCP resource protocol):**

| URI | Returns |
|-----|---------|
| `iris://inbox` | Current inbox state |
| `iris://thread/{id}` | Full thread with messages |
| `iris://contact/{email}` | Contact profile |
| `iris://stats` | Inbox statistics |

**Prompts (MCP prompt protocol):**

| URI | Description |
|-----|-------------|
| `iris://prompts/briefing` | Daily briefing prompt |
| `iris://prompts/draft-reply` | Contextual reply draft prompt |
| `iris://prompts/summarize` | Thread summary prompt |

### 2.3 Shared Types (`iris-common` crate)

Extract stable types from server into a library crate:

- Core models: Message, Thread, Account, Contact (serializable)
- Error types and common result types
- Config types
- API key / permission types

Note: Keep request/response types local to the server to avoid tight coupling on every API change. Only extract types the CLI actually needs.

Both `iris-server` and `iris` (CLI) depend on `iris-common`. Prevents type drift on shared models.

**Cargo workspace structure:**

```
iris/
├── Cargo.toml          # workspace root
├── crates/
│   ├── iris-common/    # shared types
│   ├── iris-server/    # current src/ moves here
│   └── iris-cli/       # new CLI binary
├── web/                # frontend (unchanged)
└── migrations/         # shared (unchanged)
```

**Workspace restructure is high-risk** — it touches every import path, the Dockerfile, docker-compose.yml, and migration path resolution. This requires its own detailed implementation plan as a prerequisite sub-step before any Layer 2 work begins. The plan must cover:
- Migration path resolution strategy (currently relative to binary working dir)
- Dockerfile multi-stage build adaptation
- Dev-dependencies and test harness migration
- Verification checklist (all 975 tests must pass post-restructure)
- Rollback strategy

---

## Layer 3: Showcase Features

Six features demonstrating Iris intelligence, built on the polished foundation.

### 3.1 Cross-Thread Knowledge Graph

**Purpose:** Map entities (people, projects, decisions, dates, amounts) across all threads into a queryable relationship map.

**UX:**
- New page: `/graph` — visual entity explorer
- Contact popover enrichment: shared topics and key decisions
- Chat integration: "everything involving Sarah and budget" queries the graph

**Backend:**
- `entities` table: id, canonical_name, type (person/org/project/date/amount), confidence
- `entity_aliases` table: entity_id, alias_name, source_message_id — maps name variants to canonical entity
- `entity_relations` table: entity_a, entity_b, relation_type, weight
- Entity disambiguation: email addresses anchor person entities. "Sarah", "Sarah Smith", "sarah@co.com" resolve to one canonical entity via alias table. AI extraction pipeline includes a merge step that checks existing entities before creating new ones.
- AI extraction job: runs on ingest via existing job queue
- API: `GET /api/graph?query=...` returns entity subgraph with connected threads

**Migration:** `052_knowledge_graph.sql`

### 3.2 Temporal Reasoning

**Purpose:** Understand time-relative queries. "Emails from around when we launched v2" resolves to actual dates from email context.

**UX:**
- Search bar accepts natural time expressions, shown as resolved date chip
- Chat handles temporal queries via agentic tool loop
- Can leverage knowledge graph timeline data when available, but operates independently with its own NER pipeline

**Backend:**
- Standalone temporal NER in AI pipeline: extract date references and event markers from emails
- `timeline_events` table: event_name, approximate_date, source_message_ids
- Search API extended: `temporal_query` param resolved via LLM before search
- Note: 3.2 has a soft dependency on 3.1 (can use knowledge graph data for richer event context) but functions independently. The temporal NER pipeline extracts events from email text directly.

**Migration:** `053_temporal_events.sql`

### 3.3 Writing Style Learning

**Purpose:** Analyze sent email history to learn user's voice. Drafts automatically match style.

**UX:**
- Settings > AI: "Writing Style" section showing detected traits
- Compose: AI drafts use learned style by default
- Style indicator badge in compose

**Backend:**
- Style extraction job: analyze last 200 sent emails
- `writing_style` table: user_id, trait_type, trait_value, confidence, examples
- Style prompt injection into all draft generation
- Weekly re-analysis to adapt

- Scheduling: weekly re-analysis uses a `last_run_at` check in the job worker — if `now - last_run_at > 7 days`, enqueue a style extraction job. Same pattern as existing job queue polling.

**Migration:** `054_writing_style.sql`

### 3.4 Email Delegation Agent

**Purpose:** AI handles specific email types autonomously based on configurable playbooks.

**UX:**
- Settings > AI: "Delegation Playbooks" CRUD
- Inbox: "AI handled" badge with one-click undo
- Notification: "AI drafted 3 replies and accepted 1 meeting. Review?"
- Chat: "What did you handle while I was away?"

**Backend:**
- `delegation_playbooks` table: name, trigger_conditions (JSON), action_type, action_template, enabled
- Delegation worker: matches new email against playbooks, executes with audit
- Confidence threshold: 0.85 for auto-action, otherwise queues for review
- All actions in existing audit_log

**Migration:** `055_delegation.sql`

### 3.5 Auto-Draft for Routine Emails

**Purpose:** Detect predictable routine emails and pre-draft replies.

**UX:**
- Inbox: "Draft ready" chip on routine emails
- Thread view: inline AI draft strip with one-click compose
- Settings > AI: "Auto-Draft" toggle + sensitivity slider

**Backend:**
- Pattern detection: cluster sent replies by structural similarity
- `auto_draft_patterns` table: pattern_hash, template, trigger_conditions, success_rate
- On new email: match patterns, generate draft if confidence > threshold
- Feedback loop: edit → confidence decreases; send as-is → confidence increases

**Precedence with Delegation (3.4):** Delegation playbooks run first (rule-based triggers with explicit user-configured conditions). Auto-draft runs only for emails NOT matched by any playbook. If a playbook fires, auto-draft is skipped for that email. This prevents conflicting actions on the same message.

**Migration:** `056_auto_draft.sql`

### 3.6 Evolving Auto-Categorization

**Purpose:** Dynamic categories that learn from behavior, beyond Gmail's static 5 tabs.

**UX:**
- Inbox tabs become dynamic — new tabs appear with "New" badge
- Settings > Organization: manage AI-suggested categories
- "Why this category?" click-through for explainability

**Backend:**
- `custom_categories` table: name, description, is_ai_generated, email_count
- Clustering job: weekly behavior analysis
- Category suggestion pipeline: propose → accept/reject → confidence adjustment
- Existing AI classification extended to include custom categories

**Migration:** `057_custom_categories.sql`

---

## Dependencies & Ordering

```
Layer 1.1 (transitions) ──┐
Layer 1.2 (feedback)    ──┤
Layer 1.3 (hierarchy)   ──┼── All independent, can parallelize
Layer 1.4 (keyboard)    ──┤
Layer 1.6 (components)  ──┘
         │
Layer 1.5 (AI woven) ─── depends on 1.1, 1.2, 1.6
Layer 1.7 (settings)  ─── depends on 1.6
Layer 1.8 (tokens)    ─── independent, trivial
         │
Layer 2.3 (iris-common) ─── first (workspace restructure — needs own plan)
Layer 2.1 (CLI)         ─── depends on 2.3
Layer 2.2 (MCP)         ─── independent (extends existing server, no workspace dep)
         │
Layer 3.1 (knowledge graph)    ──┐
Layer 3.2 (temporal reasoning)  ──┤── soft dep on 3.1 (uses graph data if available)
Layer 3.3 (writing style)       ──┤
Layer 3.4 (delegation agent)    ──┤── independent
Layer 3.5 (auto-draft)          ──┤── runs after 3.4 (delegation takes precedence)
Layer 3.6 (evolving categories) ──┘
```

Note: 3.2 functions independently but produces richer results when 3.1 is available. 3.5 skips emails already handled by 3.4 playbooks.

## Testing Strategy

- **Test count parity:** Each layer must maintain or increase the current 975 tests. No regressions.
- **Layer 1 (UX):** Component-level tests for new shared components (Modal, FormInput, etc.). Interaction tests for keyboard navigation and command palette.
- **Layer 2 (CLI):** Integration tests for CLI commands against a running test server. Workspace restructure requires full test suite pass before proceeding.
- **Layer 2 (MCP):** Tool schema validation tests. Round-trip tests for each new MCP tool.
- **Layer 3 (Features):** Each showcase feature requires 3+ Delphi guided cases covering positive path, edge case, and error handling.
- **Pencil prototypes:** Created during implementation for all visual changes, reviewed before code.

## Out of Scope

- Calendar integration (deferred — large dependency, not required for selected features)
- Cross-platform context (Slack/Jira — deferred)
- Mobile-specific features (web-only)
- Predictive inbox (deferred to Wave 4)
- Agent-to-Agent protocols (deferred)
- Copilot-style inline ghost text (scoped down to dropdown autocomplete — see 1.5)
- Pencil prototypes will be created during implementation, not in this spec
