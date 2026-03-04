---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: ai-on-demand
slug: fh-008-ai-on-demand
---

# Feature Handoff: AI On-Demand

## What It Does

AI on-demand provides two capabilities that users trigger explicitly: thread summarization (generates a concise summary of an email conversation) and writing assist (rewrites draft text in different tones or lengths).

## How It Works

### Thread Summarization (`src/api/ai_actions.rs`)

`POST /api/threads/{id}/summarize`:

1. Loads all messages in the thread via `MessageDetail::list_by_thread`.
2. Checks for a cached summary in the first message's `ai_summary` field. If present and non-empty, returns it immediately with `cached: true`.
3. If no cache, verifies AI is enabled and a model is configured.
4. Builds a summarization prompt via `build_summary_prompt`:
   - Header: `Thread: {subject}`
   - For each message: `Message {n} (from {name}, {date}):\n{body}\n`
   - Body is truncated to 500 characters per message.
   - Total prompt is capped at 3000 characters; remaining messages are noted as truncated.
5. Sends the prompt to Ollama with a system prompt instructing a 2-4 sentence factual summary covering key topic, current status, and action items.
6. Caches the summary in the first message's `ai_summary` column for subsequent requests.
7. Returns `{ summary, cached: false }`.

### Writing Assist (`src/api/ai_actions.rs`)

`POST /api/ai/assist` accepts `{ action, content }`:

- `content` is capped at 50,000 characters (returns 413 Payload Too Large if exceeded).
- `action` determines the system prompt:
  - `rewrite` -- clearer and more professional
  - `formal` -- formal, professional tone
  - `casual` -- casual, friendly tone
  - `shorter` -- condense while preserving key points
  - `longer` -- expand with more detail
- Invalid actions return 400 Bad Request.
- The content is sent directly to Ollama as the user prompt with the action-specific system prompt.
- Returns `{ result }` with the rewritten text.

Both endpoints check `ai_enabled` and `ai_model` from the config table and return 503 Service Unavailable if AI is not configured.

## User-Facing Behavior

- In the ThreadView page, a collapsible summary panel shows the AI-generated thread summary. The summary is generated on first click and cached thereafter.
- In the ComposeModal, an AI assist dropdown offers rewrite options (Rewrite, Make Formal, Make Casual, Make Shorter, Make Longer). Selecting an option sends the current draft text and replaces it with the AI-rewritten version.

## Configuration

Same as AI Classification:

| Config Key | Values | Description |
|---|---|---|
| `ai_enabled` | "true" / "false" | Must be true for on-demand features |
| `ai_model` | model name string | Ollama model name |

## Edge Cases and Limitations

- Summary caching uses the first message in the thread. If the thread receives new messages, the cached summary becomes stale. There is no cache invalidation mechanism.
- Long threads (20+ messages) will be truncated in the prompt due to the 3000-character total cap. The summary will note truncation but may miss important later messages.
- Per-message body truncation to 500 characters means lengthy individual messages lose detail.
- Writing assist sends the entire draft content to Ollama. For very long drafts, the model may truncate its output or produce lower quality results.
- If Ollama is unreachable, both endpoints return 502 Bad Gateway.
- The summary system prompt instructs "plain text only" but some models may still include markdown formatting.

## Common Questions

**Q: How do I clear a cached summary to regenerate it?**
A: There is no API endpoint for cache invalidation. The summary is stored in the `ai_summary` column of the first message in the thread. Manually clearing this column in the database would force regeneration on the next request.

**Q: Can I use writing assist on received emails, not just drafts?**
A: The writing assist endpoint accepts any text content. The frontend currently only exposes it in the ComposeModal, but the API is not restricted to draft text.

**Q: Why does the summary sometimes miss recent messages in a long thread?**
A: The prompt is capped at 3000 characters total. With many messages, later messages may be truncated. The summary notes this with "(remaining messages truncated)."

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Summary panel shows cached stale content | New messages added after summary was cached | Clear `ai_summary` for the thread's first message |
| Assist returns 503 | AI not enabled or model not configured | Enable AI in Settings and select a model |
| Assist returns 502 | Ollama unreachable | Check Ollama health; verify `OLLAMA_URL` |
| Assist returns 413 | Content exceeds 50,000 characters | Shorten the input text |
| Summary quality is poor | Model too small or thread context truncated | Use a larger model; check prompt length |

## Related Links

- Source: `src/api/ai_actions.rs` (summarize_thread, ai_assist, build_summary_prompt, get_assist_system_prompt)
- Ollama: `src/ai/ollama.rs`
- Frontend: ThreadView summary panel, ComposeModal AI assist dropdown
