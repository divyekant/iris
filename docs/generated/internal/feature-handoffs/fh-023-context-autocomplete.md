---
id: fh-023
type: feature-handoff
audience: internal
topic: Context-Aware Autocomplete
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/api/autocomplete.rs, web/src/components/AutocompleteDropdown.svelte, web/src/components/AutocompleteTextarea.svelte]
hermes-version: 1.0.0
---

# Feature Handoff: Context-Aware Autocomplete

## What It Does

Context-Aware Autocomplete provides real-time AI-powered text completion suggestions within the compose window. As the user types, the system sends the partial draft text and surrounding thread context to the AI and returns up to three ranked completions. The user can accept, dismiss, or ignore suggestions without interrupting their writing flow.

Unlike generic text prediction, completions are grounded in the actual thread being replied to. The AI receives the last three messages in the thread (each truncated to 500 characters) alongside the partial draft, enabling suggestions that are topically coherent with the ongoing conversation. The feature is stateless — no database writes occur; each request is independent.

## How It Works

**Endpoint**: `POST /api/ai/autocomplete`

Request body:
```json
{
  "thread_id": "thread-abc",
  "partial_text": "Thanks for sending over the proposal. I think we should",
  "cursor_position": 54,
  "compose_mode": "reply"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `thread_id` | string | Yes | ID of the thread being composed in |
| `partial_text` | string | Yes | Full text of the draft up to the cursor |
| `cursor_position` | integer | Yes | Character offset of the cursor within `partial_text` |
| `compose_mode` | string | Yes | One of: `reply`, `reply_all`, `forward`, `new` |

**Processing sequence**:
1. Validates all required fields are present. Returns 400 if any are missing.
2. Fetches the thread's last 3 messages from the `messages` table, ordered by `date` descending (most recent first). Each message body is truncated to 500 characters before inclusion in the prompt. Messages are reversed to chronological order in the prompt for natural reading.
3. Constructs a prompt that provides:
   - Thread context: the three messages with sender and truncated body.
   - Current compose mode (e.g., "You are composing a reply").
   - The partial draft text, with the cursor position marked.
   - An instruction to return a JSON array of up to 3 completion suggestions, each with `text` (the suggested continuation, not a full replacement) and `confidence` (0.0–1.0).
4. Dispatches through `ProviderPool`. Parses the JSON response.
5. Sorts suggestions by `confidence` descending.
6. Returns the suggestions along with a `debounce_ms` hint of 300.

Response:
```json
{
  "suggestions": [
    {
      "text": " schedule a call to walk through the pricing section",
      "confidence": 0.88
    },
    {
      "text": " move forward with option B as discussed",
      "confidence": 0.74
    },
    {
      "text": " loop in Maya before we finalize anything",
      "confidence": 0.61
    }
  ],
  "debounce_ms": 300
}
```

The `text` field in each suggestion is a continuation — it begins where `partial_text` ends (at `cursor_position`). The frontend appends the selected suggestion to the existing draft text rather than replacing it.

**`compose_mode` influence**: The compose mode is injected into the prompt as context for the AI. A `forward` suggestion would be tonally different from a `reply` or a `new` message. The mode does not change the endpoint logic or response structure.

**Thread context for `new` compose_mode**: When `compose_mode` is `new`, `thread_id` may be empty or null. In that case, thread context is omitted from the prompt and the AI generates generic completions based on `partial_text` alone.

**No database writes**: This endpoint is fully stateless. No tables are written to, and no result is cached. Each call produces a fresh AI response.

**Implementation files**:
- `src/api/autocomplete.rs` — route handler, prompt construction, response parsing
- `web/src/components/AutocompleteDropdown.svelte` — floating suggestion dropdown
- `web/src/components/AutocompleteTextarea.svelte` — textarea wrapper with autocomplete behavior

## User-Facing Behavior

**AutocompleteTextarea** wraps the compose textarea and intercepts keystrokes. On each input event, it debounces for 300ms (matching the `debounce_ms` hint from the API) before triggering a suggestion fetch. While the fetch is in flight, no visual change occurs — there is no loading indicator to avoid distraction.

**AutocompleteDropdown** appears below the textarea (or inline as a ghost text overlay, depending on implementation choice) when suggestions arrive. It lists up to 3 options with their text previews. Keyboard behavior:
- `Tab` or `→` accepts the top suggestion and inserts it at the cursor.
- `↑`/`↓` arrows cycle through the suggestions list.
- `Enter` (when dropdown is focused) accepts the selected suggestion.
- `Escape` dismisses the dropdown.
- Any other keypress dismisses the dropdown and continues normal typing.

**Suggestion text**: The dropdown renders only the continuation text (not the full draft), making it clear the user's text will be preserved.

**Inline ghost text**: If implemented as ghost text rather than a dropdown, the top suggestion appears in a lighter color inline after the cursor. Tab or → accepts it. This is an alternative to the dropdown that some implementations prefer for less visual disruption.

**Debounce**: The frontend implements its own 300ms debounce on input events before calling the API. The `debounce_ms` field in the response is returned as a hint but the client does not need to re-read it per request — 300ms is constant. The field exists to allow server-driven adjustment in a future iteration without a client deployment.

**Not triggered on**: Very short partial text (under ~10 characters), or when the cursor is not at the end of the text (mid-draft editing). These guards are implemented in `AutocompleteTextarea.svelte`.

## Configuration

No additional configuration required. The feature uses the shared `ProviderPool` with no autocomplete-specific settings.

| Consideration | Detail |
|---|---|
| AI required | Yes — a healthy provider is required for suggestions |
| Thread context limit | Last 3 messages, each truncated to 500 characters |
| Maximum suggestions returned | 3 |
| Debounce recommendation | 300ms (returned as `debounce_ms` in response) |
| Stateless | No DB writes; no caching |
| No migration | This feature adds no database tables |

## Edge Cases & Limitations

- **No caching**: Every keystroke-triggered fetch (after debounce) results in a new AI call. For high-volume typing, this can generate significant AI provider load. The 300ms debounce reduces call frequency, but there is no deduplication of identical requests.
- **Provider latency may exceed debounce**: If the AI provider takes more than 300ms to respond, a second request may be in flight before the first completes. The frontend should cancel or ignore the earlier in-flight request when a newer one is sent (standard debounce pattern). The backend does not have request deduplication.
- **Context truncation**: Thread messages are truncated to 500 characters each. Long emails lose their detail in the prompt. Suggestions for replies to complex long-form emails may be less coherent than for shorter threads.
- **Suggestions are continuations, not alternatives**: The returned `text` is always a continuation of `partial_text` from `cursor_position`. If the user's cursor is mid-sentence, the suggestion completes from that point. Mid-text insertion suggestions are not supported — `cursor_position` beyond the end of `partial_text` returns 400.
- **Compose mode `new` with no thread**: When composing a new message with no thread context, suggestions rely entirely on the partial text. Quality is lower than reply/reply-all modes where thread context is available.
- **AI provider unavailability**: If no provider is healthy, the endpoint returns 503. The `AutocompleteTextarea` should handle this gracefully — no suggestions appear, the user continues typing normally. No error should be surfaced to the user for a failed autocomplete request.
- **Language and tone calibration**: The AI may not match the user's writing style without fine-tuning. Suggestions may be more formal or informal than the user's draft. This is a known limitation of prompt-based autocomplete without personalization.
- **No learning or personalization**: Accepted suggestions are not stored and do not influence future suggestions. There is no feedback loop.

## Common Questions

**Q: How does autocomplete differ from AI Assist (FH-008)?**
AI Assist (FH-008) is an explicit user action: the user selects a transformation (rewrite, formal, casual, shorter, longer) and the entire draft is rewritten. Autocomplete is passive and real-time: it suggests continuations as the user types, without replacing existing text. The two features coexist in the compose window without conflict.

**Q: Why is the thread context limited to 3 messages and 500 characters each?**
The 3-message / 500-char limit balances prompt size (which affects latency and cost) against context quality. Long threads and large emails could produce prompts that exceed context windows for smaller local models (Ollama). The limit is hardcoded in `src/api/autocomplete.rs` and could be made configurable in a future iteration.

**Q: Can the user turn off autocomplete?**
The current implementation does not include a user-facing toggle. The feature is active whenever the compose window is open. A settings option to disable autocomplete is a natural follow-on but is not in scope for this implementation.

**Q: Why does the response include `debounce_ms` if it is always 300?**
`debounce_ms` is included as a server-side hint to allow the server to adjust the recommended debounce interval without a client code deployment — for example, to slow down request frequency during high load, or to reduce it for faster typing UX. In the initial implementation it is always 300; the client reads it on first load and could cache it for the session.

**Q: What happens to suggestions if the user types quickly and several requests are in flight?**
The `AutocompleteTextarea` component should cancel the previous in-flight request when a new one is triggered (AbortController pattern in the fetch call). Suggestions from stale requests are ignored even if they arrive. This prevents out-of-order suggestion display.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| No suggestions appear | AI provider unhealthy; or partial_text too short (< ~10 chars) | Check provider health in Settings; verify compose text is sufficiently long before expecting suggestions |
| POST returns 503 | No healthy AI provider in pool | Verify at least one provider is configured and reachable |
| POST returns 400 | Missing required field (`thread_id`, `partial_text`, `cursor_position`, or `compose_mode`) | Inspect request body in browser devtools Network tab |
| Suggestions are topically irrelevant | Thread context not loading (empty or missing `thread_id`) | Verify `thread_id` is being passed correctly; check that thread has at least one prior message in DB |
| Dropdown flickers or shows stale suggestions | In-flight request race condition — old request arriving after new one | Confirm `AutocompleteTextarea` is using AbortController to cancel stale requests |
| Suggestions repeat or are nearly identical | AI model not diverse in output; low temperature setting | This is a model behavior issue; consider testing with a different provider or model |
| High provider load observed | Autocomplete generating too many requests | Increase debounce interval in `AutocompleteTextarea.svelte`; consider adding minimum partial_text length guard |

## Related Links

- Backend: `src/api/autocomplete.rs`
- Frontend: `web/src/components/AutocompleteDropdown.svelte`, `web/src/components/AutocompleteTextarea.svelte`
- Related: FH-008 (AI Assist — compose-time AI, explicit transformation), FH-003 (Compose & Send — compose window host), FH-007 (AI Classification — shared ProviderPool)
- Prior handoffs: FH-016 (Multi-Reply — compose mode variants that autocomplete supports)
