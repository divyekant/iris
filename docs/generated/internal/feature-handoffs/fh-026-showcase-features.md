---
id: fh-026
type: feature-handoff
audience: internal
topic: Showcase Features — Intelligence & Automation
status: draft
generated: 2026-03-15
source-tier: direct
context-files: [
  src/api/knowledge_graph.rs,
  src/api/temporal_search.rs,
  src/api/writing_style.rs,
  src/api/auto_draft.rs,
  src/api/delegation.rs,
  src/api/categories.rs,
  web/src/components/DelegationSettings.svelte,
  web/src/components/WritingStyleSettings.svelte,
  migrations/051_knowledge_graph.sql,
  migrations/052_temporal_events.sql,
  migrations/053_writing_style.sql,
  migrations/054_auto_draft.sql,
  migrations/055_delegation_playbooks.sql,
  migrations/056_custom_categories.sql
]
hermes-version: 1.0.0
---

# Feature Handoff: Showcase Features — Intelligence & Automation

## What It Does

This handoff covers six features shipped as Wave 3 Layer 3 (Showcase). Together they form an intelligence and automation layer that learns from the user's email behavior, proactively generates drafts, delegates routine work, and evolves the inbox organization model over time.

---

## Feature 1: Knowledge Graph

The knowledge graph automatically extracts named entities — people, organizations, projects, and topics — from email bodies and builds a queryable graph of relationships and co-occurrences across threads.

**New tables** (migration 051_knowledge_graph.sql):
- `graph_entities` — `id`, `account_id`, `name`, `type` (person/organization/project/topic), `created_at`
- `graph_entity_messages` — join table: `entity_id`, `message_id` (links entities to source messages)
- `graph_entity_relations` — `entity_a_id`, `entity_b_id`, `relation_type`, `weight`

**Endpoints**:
- `POST /api/graph/extract/{message_id}` — run AI extraction on a specific message; stores new entities and relations
- `GET /api/graph?query={term}` — search entities by name/keyword; returns entity objects with `relations` and `thread_ids`

**Processing**:
1. AI prompt receives message subject + body and returns structured JSON: `{entities: [{name, type}], relations: [{a, b, type}]}`
2. Entities are deduplicated by `(account_id, LOWER(name), type)` before insert
3. Relations use `entity_a_id < entity_b_id` ordering for dedup (undirected graph)
4. Extraction is enqueued as a `knowledge_extract` job type via the job queue

**Job type**: `knowledge_extract` — added to `processing_jobs` table alongside existing types

---

## Feature 2: Temporal Reasoning

Temporal Reasoning enables natural-language date queries ("emails from last week", "messages around the product launch") and maintains a timeline of extracted calendar events across threads.

**New tables** (migration 052_temporal_events.sql):
- `timeline_events` — `id`, `message_id`, `description`, `event_date`, `event_type` (meeting/deadline/reminder/other), `extracted_at`

**Endpoints**:
- `POST /api/search/temporal` — accepts `{query: string}`; uses AI to resolve the query to a `{from, to}` date range; runs message search within that range
- `GET /api/timeline` — lists all extracted timeline events sorted by `event_date` ascending

**Date resolution**:
- AI is prompted: "Given today's date is {today}, convert this phrase to a JSON date range: {query}. Return {from: ISO, to: ISO}."
- Result is validated: `from` must precede `to`, both must be parseable ISO timestamps
- Fallback: if AI fails, returns 422 with `error: "date_resolution_failed"`

**Event extraction**: timeline events are extracted during message ingest via a `timeline_extract` job type.

---

## Feature 3: Writing Style Learning

Writing Style Learning analyzes the account holder's sent emails to extract their characteristic greeting style, sign-off, and overall tone. This profile is injected into AI compose prompts so AI-generated drafts match the user's voice.

**New table** (migration 053_writing_style.sql):
- `writing_style_profiles` — `account_id` (PK), `greeting`, `signoff`, `tone` (formal/casual/neutral), `messages_analyzed`, `updated_at`

**Endpoints**:
- `POST /api/style/analyze/{account_id}` — scans the account's Sent folder (minimum 5 messages), runs AI analysis, stores/updates the profile
- `GET /api/style/{account_id}` — returns the stored profile; 404 if not yet analyzed

**AI prompt**: sent email bodies are sampled (up to 20, latest first), truncated to 200 chars each, and included in a single prompt asking the model to identify the dominant greeting, sign-off phrase, and tone.

**Compose integration**: when `POST /api/ai/assist` or auto-draft generates body text, the style profile is fetched and injected into the system prompt: "Match this user's style: greeting={greeting}, signoff={signoff}, tone={tone}."

---

## Feature 4: Auto-Draft

Auto-Draft recognizes routine email patterns from historical sent mail and pre-generates ready-to-use draft replies for incoming messages that match those patterns. A "Draft ready" chip appears in the thread view UI.

**New tables** (migration 054_auto_draft.sql):
- `draft_patterns` — `id`, `account_id`, `description`, `trigger_keywords` (JSON array), `template_body`, `confidence`, `use_count`, `created_at`
- `auto_drafts` — `id`, `message_id`, `pattern_id`, `subject`, `body`, `confidence`, `status` (pending/used/dismissed), `created_at`

**Endpoints**:
- `GET /api/auto-draft/{message_id}` — returns pre-generated draft if one exists for the message; 404 if none
- `POST /api/auto-draft/{draft_id}/feedback` — accepts `{action: "used" | "dismissed"}`; adjusts `draft_patterns.confidence` accordingly (used: +0.05, dismissed: -0.05, clamped to [0.1, 1.0])

**Pattern learning**: a background job (`auto_draft_learn`, job queue) periodically scans sent mail to extract recurring response patterns. Pattern matching uses keyword overlap scoring against incoming message subjects.

**UI chip**: `ThreadView.svelte` checks for a pre-existing auto-draft on mount; if found, renders a "Draft ready" chip above the reply area. Clicking it opens ComposeModal pre-populated with the draft body.

---

## Feature 5: Delegation Agent

The Delegation Agent lets users define playbooks — rule sets that automatically handle specific email types. Supported actions: `auto_reply`, `draft`, `forward`, `archive`, `label`. Playbooks are evaluated for each incoming message during sync.

**New tables** (migration 055_delegation_playbooks.sql):
- `delegation_playbooks` — `id`, `account_id`, `name`, `trigger_json` (JSON: conditions + match strategy), `action_json` (JSON: type + params), `enabled`, `run_count`, `created_at`
- `delegation_executions` — `id`, `playbook_id`, `message_id`, `action_type`, `status` (executed/failed/skipped), `executed_at`

**Endpoints**:
- `POST /api/delegation/playbooks` — create a playbook
- `GET /api/delegation/playbooks` — list all playbooks for the account
- `PUT /api/delegation/playbooks/{id}` — update (enable/disable/edit)
- `DELETE /api/delegation/playbooks/{id}` — delete
- `POST /api/delegation/process/{message_id}` — evaluate all enabled playbooks against a message; execute the first match

**Trigger conditions**:
- `field`: `subject`, `sender`, `category`, `body`
- `operator`: `contains`, `equals`, `starts_with`, `regex`
- `match`: `all` (AND) or `any` (OR)

**Action types**:

| Type | Effect |
|---|---|
| `auto_reply` | Send a reply using `template` text |
| `draft` | Create a draft reply using `template` |
| `forward` | Forward to `to` address |
| `archive` | Move message to Archive folder |
| `label` | Apply `label_name` to message |

**Sync integration**: `process_message_delegation()` is called from the sync loop for each new inbound message, before AI classification.

---

## Feature 6: Evolving Categories

Evolving Categories uses clustering and AI labeling to suggest new inbox category tabs based on the user's actual email patterns. Users can accept suggestions to create custom categories that appear as dynamic tabs alongside the standard Primary/Updates/Social/Promotions tabs.

**New tables** (migration 056_custom_categories.sql):
- `custom_categories` — `id`, `account_id`, `name`, `slug`, `source` (ai_suggestion/manual), `created_at`
- `category_suggestions` — `id`, `account_id`, `name`, `description`, `message_count`, `confidence`, `status` (pending/accepted/dismissed), `created_at`

**Endpoints**:
- `POST /api/categories/analyze/{account_id}` — run clustering analysis; generate and store suggestions
- `GET /api/categories/suggestions` — list pending suggestions
- `POST /api/categories` — accept a suggestion or create a custom category manually
- `DELETE /api/categories/{id}` — remove a custom category

**Analysis process**:
1. Sample up to 100 recent messages (non-Sent, non-Spam)
2. Group by existing `ai_category` and sender domain patterns
3. Prompt AI with cluster descriptions to generate human-readable category names + rationale
4. Store suggestions with `confidence` score

**UI**: `Inbox.svelte` fetches custom categories on mount via `GET /api/categories`; renders one additional tab per custom category after the standard four. Tab click sets `activeCategory` filter; messages filtered by `ai_category = slug` or custom classification.

---

## Common Questions

**Q: Will knowledge graph extraction run automatically or only on demand?**
On demand via `POST /api/graph/extract/{message_id}`, and also enqueued automatically during message ingest (job type `knowledge_extract`). The job runs asynchronously so extraction doesn't block sync.

**Q: What happens if the AI fails to resolve a temporal query?**
`POST /api/search/temporal` returns 422 with `error: "date_resolution_failed"`. The UI should offer the user a fallback manual date picker. The FTS5 keyword portion of the query still runs if the date phrase is stripped.

**Q: How does writing style injection affect the compose flow?**
Style is fetched lazily — only when a compose or assist request is made. If no profile exists, compose proceeds without style injection (no error). Style is never injected into AI Chat tool calls.

**Q: Can delegation playbooks conflict with each other?**
Only the first matching playbook (ordered by `created_at` ascending) is executed per message. Subsequent matches are skipped. The `delegation_executions` table records the outcome for auditing.

**Q: What is the minimum message count for category analysis?**
20 messages are required. If fewer than 20 messages exist for the account, `POST /api/categories/analyze` returns 422 with `error: "insufficient_messages"`.
