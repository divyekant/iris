---
id: fh-017
type: feature-handoff
audience: internal
topic: Task Extraction
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-017: Task Extraction

## What It Does

Task Extraction uses the configured AI provider to identify actionable items within email content and return them as a structured list. Each extracted task includes a description, an optional deadline, and a priority level (high, medium, or low). The feature targets emails that contain requests, action items, or follow-up commitments that would otherwise require manual tracking.

The feature can operate on a full thread (all messages) or a single message, controlled by which identifier is supplied in the request.

## How It Works

**Endpoint**: `POST /api/ai/extract-tasks`

**Request body** (at least one ID required):
```json
{
  "thread_id": "abc123",
  "message_id": "msg456"
}
```

Supplying `thread_id` instructs the AI to analyze all messages in the thread. Supplying `message_id` restricts analysis to that single message. Supplying both is valid; `message_id` takes precedence and only that message is analyzed.

**Processing sequence**:
1. Route handler (`extract_tasks` in `src/api/ai_actions.rs`) validates that at least one of `thread_id` or `message_id` is present; returns 400 if both are absent.
2. Fetches the relevant message(s) from the database; returns 404 if the specified `message_id` does not exist.
3. Calls `build_extract_tasks_prompt` to construct a prompt containing the email body text and explicit instructions to return a JSON array of task objects.
4. Sends to the active AI provider via `ProviderPool`; returns 503 if no provider is available.
5. `parse_extracted_tasks` strips markdown fences from the response and parses the JSON array.
6. Returns the task list. Returns an empty array (`{"tasks": []}`) when the AI determines no actionable items exist.

**Response structure**:
```json
{
  "tasks": [
    {
      "description": "Send the revised budget spreadsheet to Sarah",
      "deadline": "2026-03-20",
      "priority": "high"
    },
    {
      "description": "Schedule a follow-up call with the legal team",
      "deadline": null,
      "priority": "medium"
    }
  ]
}
```

**Task fields**:

| Field | Type | Values |
|---|---|---|
| `description` | string | Free-text task description from AI |
| `deadline` | string or null | Date string if mentioned in email (format varies by model); null if no deadline found |
| `priority` | string | `"high"`, `"medium"`, or `"low"` |

**Implementation files**:
- `src/api/ai_actions.rs` — `extract_tasks`, `build_extract_tasks_prompt`, `parse_extracted_tasks`
- `web/src/components/thread/TaskList.svelte` — extracted task display with priority badges

## User-Facing Behavior

- In ThreadView, an **Extract Tasks** button appears in the thread action toolbar.
- Clicking it triggers the API call and displays a loading indicator. Extraction typically takes 2–6 seconds.
- Results appear in a `TaskList.svelte` panel within the thread view. Each task is shown as a card with:
  - The task description
  - A deadline label if present (e.g., "Due March 20")
  - A priority badge: high = red/error color, medium = yellow/warning color, low = green/success color (matching CLAUDE.md priority badge semantics)
- If no tasks are found, the panel shows an empty state message ("No action items found").
- Tasks are display-only in the current release; there is no export to a task manager or calendar integration.

## Configuration

Requires a configured and healthy AI provider. Configuration is shared with all AI features via `ProviderPool`. See FH-007 §Configuration for provider setup details.

No feature-specific flags or settings exist for task extraction.

## Error Responses

| HTTP Status | Condition |
|---|---|
| 400 | Both `thread_id` and `message_id` are absent from the request body |
| 404 | Specified `message_id` does not exist in the database |
| 503 | No AI provider configured or all providers unreachable |
| 500 | AI response could not be parsed as a task array |

Note: If `thread_id` is supplied but does not exist, behavior depends on whether any messages are found for that thread. If the message lookup returns zero results, the AI receives an empty body and will likely return `{"tasks": []}` rather than a 404. This is a known limitation — thread existence is not independently validated.

## Edge Cases & Limitations

- **`message_id` takes precedence**: When both IDs are supplied, only the single message is analyzed. The thread content is not included.
- **Empty email bodies**: If the message body is empty or contains only metadata, the AI will typically return `{"tasks": []}`. No error is raised.
- **Deadline format inconsistency**: The AI extracts deadlines from natural language ("by end of week", "March 20th"). The returned string format is not normalized — it reflects how the AI chose to express it. Downstream consumers should treat `deadline` as a human-readable string, not a parseable ISO-8601 date.
- **Priority assignment**: Priority is assigned by the AI based on urgency signals in the email text. The mapping is not deterministic — the same email run twice may produce different priority assignments, especially on smaller models.
- **Parse failures**: If the AI wraps its response in unexpected formatting beyond standard markdown fences, `parse_extracted_tasks` will fail and return 500. Retrying usually resolves transient parse errors.
- **No persistence**: Extracted tasks are not stored in the database. Each call re-invokes the AI. There is no task tracking, completion state, or history.
- **Long threads**: Very long threads with many messages may exceed provider context limits. The prompt includes full message body text; the provider may silently truncate input.

## Common Questions

**Q: Are extracted tasks saved anywhere?**
No. The current release returns tasks as a transient response only. They are displayed in the UI session but are not persisted to the database, and are not sent to any external task manager or calendar. If the user navigates away, the task list is lost. A future iteration may add persistence to a `tasks` table.

**Q: Can the user extract tasks from a single message in a multi-message thread?**
Yes. Pass `message_id` instead of (or in addition to) `thread_id`. When `message_id` is present, only that message is analyzed.

**Q: What if the email has no action items?**
The endpoint returns `{"tasks": []}` — an empty array, not an error. The UI shows the empty state message.

**Q: How accurate is task extraction?**
Accuracy varies by model. Anthropic Claude and GPT-4o-mini perform well on standard business email. Ollama with smaller models (7B/8B parameter range) may miss implied tasks or generate spurious ones. For high-stakes use (legal, contracts), users should always review the list manually.

**Q: Can I mark a task as complete?**
Not in the current release. The TaskList component is display-only. Task completion tracking is not implemented.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Returns 400 | Both `thread_id` and `message_id` are absent from the request | Ensure at least one valid ID is included in the request body |
| Returns 404 | `message_id` does not exist | Verify the message ID from the thread view; IDs are stable across sessions but change if the DB is reset |
| Returns 503 | AI provider not configured or Ollama not running | Check Settings > AI; verify Ollama health at `http://localhost:11434/api/health` |
| Returns 500 | AI response was not parseable JSON | Retry once; if persistent, check which model is active and consider switching to a larger or more instruction-following model |
| Empty task list for email with clear action items | AI missed tasks | Retry; provide `message_id` to narrow scope; or switch to a more capable model |
| Deadline shows as null even when email mentions a date | AI did not extract the date | This can occur when dates are expressed ambiguously ("next week", "ASAP"); retry with a more capable model |
| Priority badges show wrong color | Token mapping issue in `TaskList.svelte` | Verify that priority values from the API are `"high"`, `"medium"`, or `"low"` exactly; check browser console for CSS token errors |

## Related Links

- Backend: `src/api/ai_actions.rs` (`extract_tasks`, `build_extract_tasks_prompt`, `parse_extracted_tasks`)
- Frontend: `web/src/components/thread/TaskList.svelte`
- Prior handoffs: FH-007 (AI Classification), FH-008 (AI On-Demand), FH-016 (Multi-Option Reply — shares AI pipeline patterns)
- Provider configuration: FH-007 §Configuration
