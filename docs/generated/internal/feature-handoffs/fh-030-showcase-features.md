---
id: fh-030
type: feature-handoff
audience: internal
topic: showcase-features
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Feature Handoff: Showcase Features

## What It Does

This handoff covers six AI-powered features that demonstrate Iris's intelligence capabilities: delegation agent with playbooks, custom categories with AI analysis, writing style learning, auto-draft for routine emails, knowledge graph with entity extraction, and temporal reasoning for natural-language date queries. Each feature has its own API surface, database tables, and AI pipeline integration.

## How It Works

### 1. Delegation Agent (`src/api/delegation.rs`)

The delegation agent automates email handling based on configurable playbooks. Each playbook defines trigger conditions and an action to take when conditions are met.

**Trigger Conditions** (`TriggerConditions` struct):
- `sender_domain` -- Match emails from a specific domain
- `subject_contains` -- Match emails with a subject substring
- `category` -- Match emails with a specific AI-assigned category
- `intent` -- Match emails with a specific detected intent

**Action Types**: `auto_reply`, `draft_reply`, `forward`, `archive`, `label`

**Confidence Threshold**: Each playbook has a `confidence_threshold` (default 0.85). The delegation engine only acts if the match confidence meets or exceeds this value.

**API Endpoints**:
- `GET /api/delegation/playbooks` -- List all playbooks (optional `account_id` filter)
- `POST /api/delegation/playbooks` -- Create a playbook with trigger conditions, action type, and optional template
- `PUT /api/delegation/playbooks/{id}` -- Update any playbook field (partial updates supported)
- `DELETE /api/delegation/playbooks/{id}` -- Delete a playbook
- `POST /api/delegation/process/{message_id}` -- Process a message against all active playbooks
- `GET /api/delegation/actions` -- List recent delegation actions (with optional limit)
- `POST /api/delegation/actions/{id}/undo` -- Undo a delegation action
- `GET /api/delegation/summary` -- Summary stats: actions today, pending review, active playbooks

Each delegation action records the playbook that matched, the message ID, the action taken, confidence score, and status. The `delegation_process` job type in the worker processes incoming emails against playbooks automatically.

### 2. Custom Categories (`src/api/custom_categories.rs`)

Users can define custom email categories beyond the built-in ones (Primary, Updates, Social, Promotions). The system also supports AI-generated category suggestions.

**Category Fields**: `name`, `description`, `is_ai_generated`, `email_count`, `status` (active, suggested, dismissed).

**API Endpoints**:
- `GET /api/categories/custom` -- List custom categories (excludes dismissed, optional `account_id` filter)
- `POST /api/categories/custom` -- Create a category (sets `is_ai_generated = false`, `status = active`)
- `PUT /api/categories/custom/{id}` -- Update name and/or description
- `DELETE /api/categories/custom/{id}` -- Hard delete a category
- `POST /api/categories/analyze/{account_id}` -- AI analysis of 200 recent INBOX messages to suggest new categories
- `POST /api/categories/custom/{id}/accept` -- Accept a suggested category (changes status to active)
- `POST /api/categories/custom/{id}/dismiss` -- Dismiss a suggested category
- `GET /api/categories/explain/{message_id}` -- AI explanation of why a message was assigned its category

The analyze endpoint samples up to 200 recent messages, extracts subjects, senders, and existing categories, sends them to the AI provider with existing category names for context, and returns suggested new categories. Requires `ai_enabled = true` and at least one configured AI provider.

### 3. Writing Style Learning (`src/api/writing_style.rs`)

Analyzes the user's sent emails to learn writing style traits for use in AI-generated drafts.

**Trait Types**: `greeting`, `signoff`, `tone`, `avg_length`, `formality`, `vocabulary`.

Each trait has a `confidence` score (0.0-1.0) and optional `examples` (excerpts from actual sent emails).

**API Endpoints**:
- `GET /api/style/{account_id}` -- Get stored style traits for an account
- `POST /api/style/{account_id}/analyze` -- Analyze up to 200 sent emails (capped at 30 samples, ~6000 chars)

The analysis pipeline:
1. Fetches sent emails with non-empty bodies.
2. Builds a sample from the first 30 emails (body truncated to 300 chars each).
3. Sends to AI with a structured prompt requesting JSON trait extraction.
4. Parses the AI response and stores/updates traits in `writing_style_traits`.

### 4. Auto-Draft (`src/api/auto_draft.rs`)

Automatically generates draft replies for routine incoming emails using AI, incorporating the user's writing style.

**Configuration**: Enabled via `auto_draft_enabled` config key. Sensitivity levels: `conservative`, `balanced`, `aggressive`.

**API Endpoints**:
- `GET /api/auto-draft/{message_id}` -- Check for a pending auto-draft for a message
- `POST /api/auto-draft/generate/{message_id}` -- Generate an auto-draft (idempotent -- returns existing if one exists)
- `POST /api/auto-draft/{draft_id}/feedback` -- Submit feedback on an auto-draft (accept, edit, reject)
- `GET /api/config/auto-draft` -- Get auto-draft configuration
- `PUT /api/config/auto-draft` -- Update auto-draft configuration

The generation pipeline:
1. Verifies AI is enabled and auto-draft is enabled.
2. Fetches the incoming message (subject, sender, body).
3. Returns existing pending draft if one already exists (idempotent).
4. Loads the user's writing style traits from the database.
5. Sends the message + style context to the AI provider.
6. Stores the generated draft in `auto_drafts` with `status = pending`.

The `auto_draft` job type in the worker can trigger auto-draft generation for qualifying incoming emails.

### 5. Knowledge Graph (`src/api/knowledge_graph.rs`)

Extracts entities (people, organizations, projects, amounts, dates) and their relationships from email content to build a searchable knowledge graph.

**Entity Types**: `person`, `org`, `project`, `date`, `amount`.

**Relation Types**: `works_at`, `manages`, `collaborates_with`, `part_of`, `reports_to` (AI-determined).

**API Endpoints**:
- `GET /api/graph` -- Query the graph by text (filters entities by canonical name)
- `GET /api/graph/entities` -- List entities with optional type filter and limit
- `POST /api/graph/extract/{message_id}` -- Extract entities, relations, and events from a message

**Extraction Pipeline**:
1. Fetches message subject, sender, and body (truncated to 4000 chars).
2. Sends to AI with a structured extraction prompt.
3. Parses the AI response into entities, relations, and events.
4. For each entity: finds or creates by canonical name (case-insensitive). Also checks aliases. Creates entity aliases anchored to the source message.
5. For each relation: links entity A to entity B with the relation type and a weight.
6. For each event: stores in `timeline_events` with approximate date and precision.

The `entity_extract` job type in the worker runs extraction on new emails automatically.

**Entity Resolution**: `find_or_create_entity()` checks:
1. Exact match on `canonical_name` (case-insensitive) and `entity_type`.
2. Alias match via `entity_aliases` table.
3. If no match, creates a new entity and adds the canonical name as an alias.

### 6. Temporal Reasoning (`src/api/temporal.rs`)

Resolves natural-language temporal references ("emails around the product launch," "last quarter's budget discussions") to concrete date ranges, then searches emails within those ranges.

**API Endpoints**:
- `POST /api/search/temporal` -- Natural-language temporal search
- `GET /api/timeline` -- List timeline events (from knowledge graph extraction)

**Resolution Pipeline**:
1. Loads known events from `timeline_events` as context.
2. Sends the query + events context + today's date to the AI provider.
3. AI resolves to a `{start_date, end_date, description, confidence}` range.
4. Uses +/-2 week windows for approximate events, +/-1 week for precise dates.
5. Falls back to last 30 days if no temporal reference is found.
6. Queries the messages table for emails within the resolved date range.

The temporal system prompt instructs the AI to match against known timeline events first, then infer from context.

## User-Facing Behavior

- **Delegation**: Users create playbooks in the UI to automate email handling. Actions appear in the delegation log and can be undone. The summary dashboard shows daily activity.
- **Custom Categories**: Users see AI-suggested categories they can accept or dismiss. Accepted categories appear in the category filter alongside built-in ones.
- **Writing Style**: After analysis, the system knows the user's greeting style, sign-off, tone, and formality level. This feeds into auto-draft and AI compose features.
- **Auto-Draft**: When a routine email arrives, a draft reply appears automatically. Users can accept, edit, or reject it.
- **Knowledge Graph**: Entity cards show people, orgs, and projects extracted from email. Users can browse connections and see which threads mention an entity.
- **Temporal Search**: Users type natural-language queries like "emails from around the board meeting" and get results scoped to the right time period.

## Configuration

| Feature | Config Key | Default | Description |
|---|---|---|---|
| AI (global) | `ai_enabled` | `false` | Master switch for all AI features |
| Auto-draft | `auto_draft_enabled` | `false` | Enable/disable auto-draft generation |
| Auto-draft sensitivity | `auto_draft_sensitivity` | `balanced` | `conservative` / `balanced` / `aggressive` |
| Playbook threshold | Per-playbook | `0.85` | Minimum confidence to trigger a playbook |

## Edge Cases & Limitations

- **All features require AI.** If no AI provider is configured or `ai_enabled` is false, analyze/extract/generate endpoints return 503.
- **Writing style analysis is best-effort.** Small sent folders (<10 emails) produce low-confidence traits.
- **Auto-draft is idempotent.** Calling generate twice for the same message returns the existing draft.
- **Entity resolution is case-insensitive but type-sensitive.** "Acme" as `org` and "Acme" as `project` are separate entities.
- **Temporal resolution depends on timeline events.** Queries referencing events not yet in the timeline fall back to the 30-day default.
- **Playbook trigger conditions are AND-logic.** All specified conditions must match (conditions set to `None` are ignored).
- **Category analysis samples 200 messages.** Accounts with fewer messages get less accurate suggestions.
- **AI responses must parse as valid JSON.** Markdown-fenced JSON is stripped, but malformed AI output silently fails.

## Common Questions

**Q: How does auto-draft use writing style?**
A: When generating a draft, the system loads all stored style traits for the account and includes them in the AI prompt. The AI uses the greeting, sign-off, tone, and formality traits to match the user's natural writing voice.

**Q: Can I have playbooks across multiple accounts?**
A: Each playbook is scoped to an `account_id`. Create separate playbooks for each account.

**Q: What triggers entity extraction?**
A: The `entity_extract` job type runs on new emails via the job queue. It can also be triggered manually via `POST /api/graph/extract/{message_id}`.

**Q: How precise is temporal reasoning?**
A: It depends on the AI model and the available timeline events. With known events, the AI anchors dates accurately. Without context, it uses broad heuristics. The `confidence` field in the response indicates how certain the AI is.

**Q: Why does category analysis cap at 200 messages?**
A: To keep AI prompt size manageable. 200 messages provide sufficient statistical signal for category pattern detection without exceeding model context limits.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| 503 on any analyze/generate endpoint | AI not enabled or no providers configured | Set `ai_enabled = true` and configure at least one provider. |
| Playbook never triggers | Confidence below threshold, or trigger conditions too specific | Lower `confidence_threshold` or broaden trigger conditions. |
| Auto-draft produces generic text | Writing style not analyzed yet | Run `POST /api/style/{account_id}/analyze` first. |
| Entity extraction finds nothing | Email body too short or no identifiable entities | Check the email has substantive content (>50 words). |
| Temporal search returns wrong date range | No matching timeline events and AI guessed poorly | Add timeline events via entity extraction, or use explicit `since/until` params. |
| Category suggestions are poor quality | Too few messages in inbox | Wait until at least 50+ messages are synced before analyzing. |

## Related

- [fh-026-showcase-features.md](fh-026-showcase-features.md) -- Earlier showcase features reference
- [fh-007-ai-classification.md](fh-007-ai-classification.md) -- AI classification pipeline
- [fh-008-ai-on-demand.md](fh-008-ai-on-demand.md) -- On-demand AI features
- [fh-028-memories-v5.md](fh-028-memories-v5.md) -- Memories integration (temporal search uses `since/until`)
- [fh-013-job-queue.md](fh-013-job-queue.md) -- Job queue (entity_extract, auto_draft, delegation_process jobs)
