---
id: fh-016
type: feature-handoff
audience: internal
topic: Multi-Option Reply
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-016: Multi-Option Reply

## What It Does

Multi-Option Reply uses the configured AI provider to generate three distinct reply drafts for a given email thread, each in a different tone: formal, casual, and brief. The user selects one of the three options, which is then loaded into the compose window for editing and sending. The feature reduces the cognitive overhead of drafting replies from scratch while giving users tonal control.

## How It Works

**Endpoint**: `POST /api/ai/multi-reply`

**Request body**:
```json
{
  "thread_id": "abc123",
  "context": "mention that we need the report by Friday"
}
```

The `context` field is optional free-text guidance that is injected into the AI prompt to steer the generated replies. If omitted, replies are based solely on the thread content.

**Processing sequence**:
1. Route handler (`multi_reply` in `src/api/ai_actions.rs`) validates that `thread_id` is non-empty and non-whitespace; returns 400 otherwise.
2. Fetches the thread and its messages from the database; returns 404 if the thread does not exist.
3. Calls `build_multi_reply_prompt` to construct a structured prompt that includes the thread body and instructs the AI to return exactly three JSON objects (one per tone).
4. Sends the prompt to the active AI provider via `ProviderPool`. Returns 503 if no AI provider is configured or healthy.
5. Receives the raw text response and passes it to `parse_multi_reply_response`, which:
   - Strips markdown code fences (` ```json ` / ` ``` `) if present in the AI output
   - Parses the result as a JSON array of three objects
6. Returns the three options to the client.

**Response structure**:
```json
{
  "options": [
    {"tone": "formal",  "subject": "Re: Q4 Budget Review", "body": "Dear Sarah, ..."},
    {"tone": "casual",  "subject": "Re: Q4 Budget Review", "body": "Hey Sarah, ..."},
    {"tone": "brief",   "subject": "Re: Q4 Budget Review", "body": "Sarah — ..."}
  ]
}
```

The response always contains exactly 3 options in the order: formal, casual, brief.

**Implementation files**:
- `src/api/ai_actions.rs` — `multi_reply`, `build_multi_reply_prompt`, `parse_multi_reply_response`
- `web/src/components/compose/MultiReplyPicker.svelte` — card-based option selector

## User-Facing Behavior

- In ThreadView, an **AI Reply Options** button appears in the reply toolbar.
- Clicking it sends the request and shows a loading spinner while the AI generates options. Generation typically takes 2–8 seconds depending on the provider and model.
- Three tone cards appear side by side:
  - **Formal** — gold-tinted card
  - **Casual** — blue-tinted card
  - **Brief** — green-tinted card
- Each card shows the subject line and a preview of the body. Clicking a card loads the full body and subject into the compose window for editing before sending.
- The user can regenerate options (re-sends the API request) or dismiss the picker and compose manually.

## Configuration

Multi-Option Reply requires a configured and healthy AI provider. At least one of the following must be present:

| Provider | Required Config |
|---|---|
| Anthropic | `ANTHROPIC_API_KEY` env var or API key saved in Settings |
| OpenAI | `OPENAI_API_KEY` env var or API key saved in Settings |
| Ollama | Ollama running locally (default `http://localhost:11434`); any model loaded |

If no provider is available, the endpoint returns 503. Provider configuration is shared with all other AI features — no feature-specific settings exist.

## Error Responses

| HTTP Status | Condition |
|---|---|
| 400 | `thread_id` is missing, empty, or whitespace-only |
| 404 | Thread does not exist in the database |
| 503 | No AI provider is configured or all providers are unreachable |
| 500 | AI returned a response that could not be parsed as three-option JSON |

## Edge Cases & Limitations

- **Parse failures**: If the AI returns malformed JSON or fewer than 3 objects, `parse_multi_reply_response` fails and the endpoint returns 500. This is more likely with Ollama models that do not follow structured output reliably. Retrying usually produces a parseable response.
- **Markdown fence stripping**: The parser strips ` ```json ` and ` ``` ` wrappers but does not handle other markdown artifacts. If a model outputs additional prose before or after the JSON array, parsing will fail.
- **Context length**: Very long threads may exceed the AI provider's context window. The prompt includes thread body text; extremely long threads (100+ messages) may be truncated silently by the provider or cause a 500 from the provider's API.
- **Subject line generation**: The AI generates reply subject lines. For threads with a clean `Re:` subject, the AI typically preserves it. For ambiguous subjects, the AI may generate a new subject — users should review before sending.
- **No caching**: Each request to the endpoint triggers a new AI generation. Results are not cached. Generating options twice for the same thread will produce different results.
- **Tone accuracy**: "Formal," "casual," and "brief" are tone labels in the prompt. Actual tone variation depends on the model. Smaller Ollama models may produce less differentiated options than Anthropic Claude or GPT-4o.

## Common Questions

**Q: Can the user request more than 3 options, or change the tones?**
Not in the current release. The API always returns exactly 3 options in the three fixed tones. Custom tone selection is not supported.

**Q: Does the context field affect all three options or just one?**
The `context` field is injected into the shared prompt and influences all three generated replies. Each tone card will reflect the guidance, albeit expressed differently per tone.

**Q: What happens if the user edits the selected reply option?**
Once a card is clicked, the content is loaded into the standard ComposeModal. The user can edit freely before sending. The original AI-generated text is not preserved separately; edits are treated as a normal draft.

**Q: Does the reply automatically thread correctly (In-Reply-To headers)?**
Yes. Selecting an option loads the content into the compose window with the thread context pre-populated, so the resulting reply will have the correct `In-Reply-To` and `References` headers — the same behavior as clicking the manual Reply button.

**Q: Why does the endpoint sometimes return 500 on Ollama?**
Smaller local models often produce responses that include markdown fences, trailing prose, or incomplete JSON. The `parse_multi_reply_response` function handles fences but not all output variants. Switching to a larger model (e.g., `llama3:70b` instead of `llama3:8b`) significantly improves parse reliability.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Returns 503 | No AI provider configured or Ollama not running | Verify AI provider in Settings > AI; check Ollama health at `http://localhost:11434/api/health` |
| Returns 500 after AI responds | AI output was not valid 3-option JSON | Retry the request; if persistent, check provider model and consider switching to a more capable model |
| Options take >30s to appear | AI provider is slow (large model, cold start) | Normal for Ollama with large models on first inference; subsequent calls faster once model is loaded |
| All three options look identical | Model did not differentiate tones | Switch to a more capable model; or provide a `context` hint to push differentiation |
| Formal card is not gold | Token/CSS not loading for `MultiReplyPicker.svelte` | Check browser console for CSS errors; verify `web/src/tokens.css` is loaded |
| Returns 404 for valid thread | Thread ID format mismatch | Thread IDs are derived from email headers and may differ from UI display IDs; confirm the correct `thread_id` is being passed |

## Related Links

- Backend: `src/api/ai_actions.rs` (`multi_reply`, `build_multi_reply_prompt`, `parse_multi_reply_response`)
- Frontend: `web/src/components/compose/MultiReplyPicker.svelte`
- Prior handoffs: FH-007 (AI Classification), FH-008 (AI On-Demand — AI Assist in compose), FH-009 (AI Chat)
- Provider configuration: FH-007 §Configuration (shared ProviderPool setup)
