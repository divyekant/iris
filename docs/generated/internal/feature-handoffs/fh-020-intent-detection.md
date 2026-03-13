---
id: fh-020
type: feature-handoff
audience: internal
topic: Intent Detection
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/api/intent.rs, web/src/components/IntentBadge.svelte, migrations/024_intent.sql]
hermes-version: 1.0.0
---

# Feature Handoff: Intent Detection

## What It Does

Intent Detection classifies individual emails into one of seven discrete categories based on the content and structure of the message: `action_request`, `question`, `fyi`, `scheduling`, `sales`, `social`, or `newsletter`. Classification is performed on demand by the configured AI provider via ProviderPool, and the result is stored persistently on the `messages` table alongside a confidence score. Once classified, the intent is retrievable without re-running the AI.

In the inbox, classified messages display an `IntentBadge` component that renders the detected intent with color coding appropriate to the category. This helps users visually triage messages at a glance before opening them.

## How It Works

**Endpoints**:
- `POST /api/ai/detect-intent` — classify an email and store the result
- `GET /api/messages/{id}/intent` — retrieve stored intent for a message

**POST /api/ai/detect-intent**

Request body:
```json
{
  "message_id": "abc123"
}
```

Processing sequence:
1. Validates `message_id` is present and non-empty.
2. Fetches the message body and subject from the `messages` table.
3. Constructs a prompt instructing the AI to classify the email into exactly one of the seven categories and return a JSON object with `intent` and `confidence` (0.0–1.0).
4. Dispatches the prompt through `ProviderPool` (round-robin, with fallback). The prompt enforces strict JSON output; the handler parses the response and extracts `intent` and `confidence`.
5. Validates that the returned `intent` string is one of the seven accepted values. If the AI returns an unrecognized value, the handler logs a warning and stores `fyi` as a safe default with `confidence` set to 0.0.
6. Writes `intent` and `intent_confidence` to the `messages` table for the given `message_id`.
7. Returns the stored values.

Response:
```json
{
  "message_id": "abc123",
  "intent": "action_request",
  "intent_confidence": 0.91
}
```

**GET /api/messages/{id}/intent**

Returns the stored intent and confidence for a message without invoking the AI. Returns `404` if the message does not exist. Returns the intent fields even if they are null (i.e., classification has not been run yet).

Response when classified:
```json
{
  "message_id": "abc123",
  "intent": "action_request",
  "intent_confidence": 0.91
}
```

Response when not yet classified:
```json
{
  "message_id": "abc123",
  "intent": null,
  "intent_confidence": null
}
```

**Migration 024**: Adds `intent TEXT` and `intent_confidence REAL` columns to the `messages` table. Both columns are nullable. No backfill is applied — existing messages have null intent until explicitly classified via the POST endpoint.

**Intent categories**:

| Intent | Description |
|---|---|
| `action_request` | Sender is explicitly asking the recipient to do something |
| `question` | Sender is asking for information or clarification |
| `fyi` | Informational message with no response or action expected |
| `scheduling` | Message concerns meeting or event scheduling, time coordination |
| `sales` | Commercial pitch, product promotion, or vendor outreach |
| `social` | Personal or social message with no work-related request |
| `newsletter` | Bulk or subscription-based informational content |

**Implementation files**:
- `src/api/intent.rs` — route handlers, prompt construction, response parsing
- `migrations/024_intent.sql` — schema migration
- `web/src/components/IntentBadge.svelte` — inbox badge component

## User-Facing Behavior

**IntentBadge** renders inline in the inbox message list row, positioned after the sender name or subject depending on layout. It is only shown when `intent` is non-null. Badge appearance by category:

| Intent | Color token | Label |
|---|---|---|
| `action_request` | `--iris-color-error` | Action |
| `question` | `--iris-color-warning` | Question |
| `fyi` | `--iris-color-text-faint` | FYI |
| `scheduling` | `--iris-color-info` | Schedule |
| `sales` | `--iris-color-text-faint` | Sales |
| `social` | `--iris-color-success` | Social |
| `newsletter` | `--iris-color-text-faint` | Newsletter |

The badge is not interactive — it does not filter or sort on click. It is read-only display.

Classification is not triggered automatically during sync. The POST endpoint must be called explicitly. Typical integration patterns are: the inbox frontend calling detect-intent for visible messages lazily (after initial render), or the job queue enqueuing classification as a background task post-sync.

## Configuration

Intent detection uses the shared `ProviderPool`. No intent-specific configuration exists. The AI provider used (Ollama, Anthropic, or OpenAI) is determined by the global provider configuration in Settings.

| Consideration | Detail |
|---|---|
| AI required | Yes — at least one provider must be healthy |
| Prompt format | JSON output enforced in system prompt |
| Retry behavior | ProviderPool handles fallback across providers |
| Backfill on migration | None — existing messages retain null intent |

## Edge Cases & Limitations

- **No auto-classification on sync**: Classification is not triggered by email sync. Emails received after migration 024 will have null intent until the POST endpoint is called. A job queue integration can be added but is not included in the initial implementation.
- **AI provider required**: If no provider is healthy (Ollama down, API keys invalid), the POST endpoint returns 503. The GET endpoint is unaffected since it reads from the database.
- **One intent per message**: The model is forced to a single classification. Multi-intent emails (e.g., a message that is both a question and a scheduling request) are assigned the most prominent intent as determined by the AI.
- **Confidence is AI-reported**: The confidence value is extracted from the AI response and stored verbatim. It reflects the model's self-reported certainty, not an empirically validated probability. Treat it as a relative signal, not an absolute measure.
- **Default fallback on parse failure**: If the AI returns malformed JSON or an unrecognized intent string, the handler falls back to `fyi` with `confidence=0.0`. The original AI response is logged at WARN level for debugging.
- **No re-classification trigger**: There is no mechanism to detect that a message has been updated and re-run classification. If message content changes (e.g., after a re-sync), the stored intent is not invalidated automatically.
- **Confidence not shown in UI**: The `IntentBadge` component renders the intent label only. The `intent_confidence` value is stored but not surfaced to the user in the current implementation.

## Common Questions

**Q: Why is classification not run automatically during sync?**
Running AI classification synchronously during IMAP sync would block the sync loop and introduce significant latency for high-volume inboxes. The intentional design separates sync (fast, database-only) from AI enrichment (slower, provider-dependent). Classification should be triggered asynchronously via the job queue or on-demand from the frontend.

**Q: What happens if the AI provider is slow or unavailable?**
The POST endpoint returns 503 if no provider in the pool is healthy. Partial success is not possible — the endpoint either classifies and stores, or returns an error without modifying the database. The GET endpoint is always available regardless of provider health.

**Q: Can a user override the detected intent?**
No. There is no manual override endpoint in the current implementation. The intent is determined by AI and stored. If the classification is wrong, the only recourse is to call the POST endpoint again (which will re-run classification and overwrite the stored value).

**Q: How is `newsletter` distinguished from `sales`?**
The AI is prompted to distinguish between bulk subscription-based content (newsletter) and direct commercial outreach targeting the recipient specifically (sales). In practice, the boundary is fuzzy. Both categories display the `--iris-color-text-faint` badge, so visual distinction in the inbox is not a goal. The categories serve primarily for filtering and analytics use cases.

**Q: Will existing messages from before migration 024 ever get classified?**
Not automatically. A backfill job would need to be built and run explicitly. The GET endpoint will return null intent for all pre-migration messages until classification is triggered on each one individually.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| POST returns 503 | No AI provider healthy | Check Settings AI configuration; verify at least one provider (Ollama/Anthropic/OpenAI) is reachable |
| POST returns 400 | `message_id` missing or empty in request body | Verify request body is valid JSON with `message_id` field |
| POST returns 404 | `message_id` not found in `messages` table | Confirm the message has been synced; check account and message ID |
| Intent stored as `fyi` with confidence 0.0 | AI returned malformed JSON or unrecognized intent string | Check `iris-server` logs at WARN level for the raw AI response; may indicate a prompt/model compatibility issue |
| IntentBadge not rendering in inbox | `intent` is null (not yet classified) | Trigger POST /api/ai/detect-intent for the message; badge only renders for non-null intents |
| IntentBadge color incorrect | CSS token not resolving | Check browser console for CSS variable warnings; verify `web/src/tokens.css` is loaded |
| GET returns 404 | Message does not exist in DB | Confirm message_id is correct and message has synced |

## Related Links

- Backend: `src/api/intent.rs`
- Migration: `migrations/024_intent.sql`
- Frontend: `web/src/components/IntentBadge.svelte`
- Related: FH-007 (AI Classification — shares ProviderPool dispatch pattern), FH-013 (Job Queue — async classification integration)
- Prior handoffs: FH-003 (Email Reading — message model), FH-005 (Inbox Management — inbox row rendering)
