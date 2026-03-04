---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: ai-classification
slug: fh-007-ai-classification
---

# Feature Handoff: AI Classification

## What It Does

AI classification automatically analyzes each incoming email using a local Ollama LLM and extracts structured metadata: intent, priority, category, summary, named entities, and deadline. The classification runs as a background pipeline during email sync and stores results in the messages table.

## How It Works

### Pipeline (`src/ai/pipeline.rs`)

The `process_email` function:

1. Truncates the email body to 2000 characters (char-safe for multi-byte UTF-8).
2. Constructs a prompt: `From: {from}\nSubject: {subject}\n\n{body_truncated}`.
3. Sends the prompt to Ollama with a system prompt that instructs the model to respond with a JSON object.
4. If user feedback patterns exist (corrections with count >= 2), appends them to the system prompt as classification hints.
5. Parses the JSON response into `AiMetadata`.

### System Prompt

The system prompt instructs the model to return a JSON object with:

- `intent`: one of ACTION_REQUEST, INFORMATIONAL, TRANSACTIONAL, SOCIAL, MARKETING, NOTIFICATION
- `priority_score`: float 0.0-1.0 (1.0 = most urgent)
- `priority_label`: one of "urgent", "high", "normal", "low"
- `category`: one of "Primary", "Updates", "Social", "Promotions", "Finance", "Travel", "Newsletters"
- `summary`: 1-2 sentence summary
- `entities`: object with arrays for people, dates, amounts, topics
- `deadline`: ISO date string if mentioned, null otherwise

### JSON Extraction

The `extract_json` function handles models that wrap JSON in markdown code blocks (`\`\`\`json ... \`\`\``). It tries three extraction strategies in order: json code block, generic code block, raw JSON object delimiters (`{` ... `}`).

### Concurrency Control

AI classification tasks are spawned as background Tokio tasks during sync, limited by a semaphore (`MAX_AI_CONCURRENCY = 4`). This prevents overwhelming the Ollama instance when syncing many messages.

### Feedback Loop (`src/api/ai_feedback.rs`)

Users can correct AI classifications via `PUT /api/messages/{id}/ai-feedback`:

- Accepts `field` (category, priority_label, or intent) and `value` (validated against allowed values).
- Updates the message's AI field directly.
- Records the correction in the `ai_feedback` table (message_id, field, original_value, corrected_value).

The `build_feedback_context` function queries correction patterns (grouped by field/original/corrected, with count >= 2) and formats them as a system prompt suffix. This is appended to the classification prompt during sync, so the model learns from repeated user corrections.

### Feedback Stats

`GET /api/ai/feedback-stats` returns:
- `total_corrections` -- total count of user corrections
- `by_field` -- correction count grouped by field
- `common_corrections` -- top 20 correction patterns (field, original, corrected, count)

## User-Facing Behavior

- After sync, messages display priority badges (urgent/high/normal/low) and category pills in the inbox list.
- The AI settings page allows enabling/disabling AI, selecting the Ollama model, and testing the connection.
- Users can click on a priority badge or category pill to correct the AI classification.
- Correction patterns influence future classifications when the same pattern is observed 2+ times.

## Configuration

AI classification is controlled via the `config` table:

| Config Key | Values | Description |
|---|---|---|
| `ai_enabled` | "true" / "false" | Master toggle for all AI features |
| `ai_model` | model name string | Ollama model to use (e.g., "llama3.2", "mistral") |

Environment:

| Variable | Default | Description |
|---|---|---|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint |

## Edge Cases and Limitations

- Classification requires Ollama to be running and a model to be loaded. If Ollama is unreachable, classification is silently skipped.
- The model may return malformed JSON, especially with smaller models. `parse_ai_response` returns None in this case, and the message is left without AI metadata.
- Body truncation to 2000 characters means long emails may lose important context in the truncated portion.
- Entity extraction quality depends on the model. Smaller models may miss entities or hallucinate.
- The feedback loop only influences classification when the same correction pattern appears 2+ times. Single corrections are recorded but do not affect future prompts.
- Only three fields support user correction: category, priority_label, and intent. Summary and entities cannot be corrected.
- The semaphore limits concurrent classification to 4 tasks. On large syncs, this creates a queue of pending classification tasks.

## Common Questions

**Q: What Ollama models work best for classification?**
A: Models with good JSON output capabilities work best. Llama 3.2 (8B), Mistral 7B, and similar instruction-tuned models produce reliable results. Smaller models (1-3B) may struggle with consistent JSON formatting.

**Q: How long does classification take per message?**
A: Depends on the model and hardware. On Apple Silicon (M-series), typical times range from 1-5 seconds per message with 7-8B parameter models. The semaphore ensures at most 4 messages are processed concurrently.

**Q: Can I re-classify all existing messages?**
A: There is no bulk re-classification endpoint. AI classification runs only during sync. A server restart triggers re-sync, but INSERT OR IGNORE means existing messages are skipped. To re-classify, you would need to clear the AI metadata fields and trigger a resync.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| No AI badges on messages | AI disabled or model not set | Check `ai_enabled` and `ai_model` in Settings |
| "Failed to parse AI response" in logs | Model returned invalid JSON | Try a different model; check the raw response in debug logs |
| Classifications seem random | Model too small or prompt too complex | Use a larger model (7B+) for better accuracy |
| Feedback corrections not taking effect | Pattern needs 2+ occurrences | Make the same correction on multiple messages |
| AI processing backed up during sync | Semaphore queue full | Normal behavior; tasks process as permits become available |

## Related Links

- Source: `src/ai/pipeline.rs`, `src/ai/ollama.rs`, `src/api/ai_feedback.rs`, `src/api/ai_config.rs`
- Sync integration: `src/imap/sync.rs` (spawn_ai_processing)
- Database: `migrations/004_ai_feedback.sql`
- Frontend: AI badges, category pills, AI Settings page
