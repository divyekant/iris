---
id: fh-024
type: feature-handoff
audience: internal
topic: Bulk Operations via Chat
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/ai/tools.rs, src/api/chat.rs, web/src/components/BulkActionCard.svelte]
hermes-version: 1.0.0
---

# Feature Handoff: Bulk Operations via Chat

## What It Does

Bulk Operations via Chat extends the agentic chat system to execute multi-message inbox actions in response to natural language instructions. A user can type "archive all unread newsletters from this week" or "mark everything from sales@vendor.com as read" and the AI agent will identify the target messages, confirm the scope with the user, and execute the operation on approval.

The feature adds a single new tool — `bulk_update_emails` — to the agentic chat tool set defined in `src/ai/tools.rs`. It integrates with the existing ACTION_PROPOSAL confirmation flow, requiring user approval before applying changes. A hard cap of 500 messages per operation prevents runaway bulk actions.

## How It Works

**Architecture**: No new API endpoint is added. Bulk operations execute within the existing `POST /api/ai/chat` agentic loop defined in `src/api/chat.rs`. The `bulk_update_emails` tool is added to the tool registry available to the LLM during the agentic loop.

**New tool: `bulk_update_emails`**

Tool definition (added to `src/ai/tools.rs`):

| Parameter | Type | Required | Description |
|---|---|---|---|
| `message_ids` | array of strings | Yes | List of message IDs to act on (max 500) |
| `action` | string | Yes | One of the 7 supported actions (see below) |
| `category` | string | Conditional | Required when action is `move_to_category` |

Supported actions:

| Action | Effect |
|---|---|
| `archive` | Moves messages to the Archive folder |
| `mark_read` | Sets `is_read = true` |
| `mark_unread` | Sets `is_read = false` |
| `trash` | Moves messages to the Trash folder |
| `star` | Marks messages as starred |
| `unstar` | Removes star |
| `move_to_category` | Sets the `category` field on messages; requires `category` parameter |

**Safety cap**: If `message_ids` contains more than 500 entries, the tool call is rejected by the handler with an error returned to the agentic loop. The LLM is expected to acknowledge the cap and reduce scope if needed.

**Agentic LLM workflow**:

The intended multi-step flow the LLM follows when a user issues a bulk instruction:
1. **Identify**: Call `list_emails` or `search_emails` (existing tools) with filters matching the user's description to enumerate candidate messages.
2. **Confirm scope**: Return a message to the user showing the count and a sample of the identified messages (e.g., "Found 23 unread newsletters from this week — proceed with archiving?"). This triggers the `BulkActionCard` confirmation UI component.
3. **Execute**: On user approval (user sends a confirmation like "yes" or "go ahead"), call `bulk_update_emails` with the collected `message_ids` and the target action.

This pattern follows the existing ACTION_PROPOSAL confirmation pattern established in the V8 AI Chat system (FH-009). The `BulkActionCard` component renders the confirmation step with a summary of what will be changed.

**Confirmation enforcement**: The agentic loop in `src/api/chat.rs` checks whether the last user message constitutes approval before proceeding with tool calls that have side effects. If the LLM attempts to call `bulk_update_emails` without having first shown the user a confirmation, the system prompt includes an instruction to always confirm before executing bulk destructive actions.

**Execution**: Once `bulk_update_emails` is called, the handler in `src/ai/tools.rs` (or `src/api/chat.rs`) executes the action against the `messages` table using a batched SQL UPDATE or the same logic as the existing batch update API (FH-005). The tool returns a result indicating how many messages were successfully updated.

Tool result returned to the LLM:
```json
{
  "updated": 23,
  "failed": 0,
  "action": "archive"
}
```

The LLM surfaces this result to the user in natural language (e.g., "Done — 23 newsletters archived.").

**Implementation files**:
- `src/ai/tools.rs` — `bulk_update_emails` tool definition and handler; updated tool registry
- `src/api/chat.rs` — no structural changes; tool is registered in the existing tool list
- `web/src/components/BulkActionCard.svelte` — confirmation UI card rendered in the chat panel

## User-Facing Behavior

**Chat interaction pattern**:

1. User types: "Archive all unread newsletters from the past 7 days."
2. AI identifies matching messages via `list_emails` tool call.
3. `BulkActionCard` renders in the chat panel:
   - Header: "Archive 18 emails"
   - Message summary: sender list or category label and date range
   - "Confirm" and "Cancel" buttons
4. User clicks "Confirm" (or types "yes").
5. AI calls `bulk_update_emails`.
6. AI responds: "Done — 18 emails archived."

**BulkActionCard** is a read-only informational card. It does not issue the API call directly — the AI agent does so after receiving the user's text or button confirmation. The "Confirm" button sends a confirmation message into the chat thread, which the AI processes in the next agentic loop iteration.

**"Cancel" behavior**: Clicking Cancel sends a cancellation message into the chat. The AI acknowledges it and does not call `bulk_update_emails`.

**Error display**: If the bulk operation partially fails (some `message_ids` not found or already in target state), the AI reports the `failed` count from the tool result. The chat does not surface individual failure details unless the count is small enough for the AI to enumerate.

**500-message cap in UI**: If the user's request would exceed 500 messages (e.g., "archive all email"), the AI identifies this from the `list_emails` result count and informs the user of the limit before requesting confirmation. The user can narrow the filter or proceed in batches.

## Configuration

No additional configuration required. Bulk chat operations use the shared `ProviderPool` for agentic loop LLM calls. No new settings are exposed.

| Consideration | Detail |
|---|---|
| Safety cap | 500 messages maximum per `bulk_update_emails` call |
| Confirmation required | Yes — ACTION_PROPOSAL pattern enforced via system prompt |
| AI required | Yes — agentic loop uses ProviderPool |
| No migration | No new database tables |
| Existing batch API reuse | Bulk action execution reuses logic from existing batch update handlers |

## Edge Cases & Limitations

- **LLM may skip confirmation**: The confirmation requirement is enforced via system prompt instruction, not a hard code gate. A sufficiently unusual or strongly-worded user prompt could cause the LLM to call `bulk_update_emails` without a prior confirmation step. A future hardening pass could add a pre-execution check in the tool handler that verifies confirmation was shown.
- **500-message cap is per tool call**: A user could issue multiple bulk requests in sequence, each under the cap, to affect more than 500 messages. The cap is not a session-level or per-user rate limit.
- **`message_ids` sourced from prior tool calls**: The LLM is responsible for collecting message IDs from `list_emails` or `search_emails` before calling `bulk_update_emails`. If the LLM hallucinates message IDs (IDs not present in the database), those IDs will silently fail (not found) and be counted in `failed`. The tool result returns `failed` count but not the failing IDs.
- **`move_to_category` category values not validated by the tool**: The `category` parameter is passed through to the SQL update. If the LLM supplies an invalid category string, the database update may succeed but set an unrecognized category value. Validation against the allowed category set should be added to the tool handler.
- **Undo not supported**: Bulk operations are not reversible through the chat interface. Archived or trashed messages can be recovered via normal inbox/folder navigation, but the chat agent does not support "undo" commands.
- **Action is applied uniformly**: All messages in `message_ids` receive the same action. The tool does not support conditional actions (e.g., "archive newsletters but star important ones from the same set") in a single call.
- **No audit trail specific to bulk chat ops**: The audit log (from FH-010) records message state changes but does not capture that the change originated from a chat bulk operation vs. a direct UI action.

## Common Questions

**Q: Why is confirmation not enforced in code rather than via system prompt?**
The current implementation delegates confirmation responsibility to the LLM via the system prompt, consistent with the ACTION_PROPOSAL pattern used throughout the chat system (FH-009). A code-level gate would require the backend to track conversation state and determine whether a confirmation was exchanged — a meaningful architectural addition. System-prompt enforcement is simpler and sufficient for the current risk level given that operations are reversible (archive/trash are not permanent deletions in typical email systems).

**Q: What if the user says "yes archive them all" in the initial message — no separate confirmation step?**
The system prompt instructs the LLM to always run a search/list step first to establish scope before executing. An initial all-in-one request should still trigger the list → confirm → execute sequence. If the LLM is well-calibrated, it will present a scope summary before acting. This behavior depends on model quality and the specificity of the system prompt.

**Q: Does `trash` permanently delete messages?**
No. `trash` moves messages to the Trash folder by updating the `folder` field on the `messages` table. Permanent deletion is a separate operation (if implemented). Messages in Trash remain in the database until explicitly purged.

**Q: Can the agent act on messages across multiple accounts?**
`list_emails` and `search_emails` support account filtering. If the user's request spans multiple accounts, the LLM would need to issue multiple list calls (one per account) and aggregate the message IDs before calling `bulk_update_emails`. This multi-account flow is not explicitly scripted in the system prompt — whether the LLM handles it depends on the model's reasoning ability.

**Q: Is there a way to preview which emails will be affected without confirming?**
Yes — the `BulkActionCard` renders a summary of the identified messages (sender, subject, count) before the user confirms. This serves as the preview step. The user can inspect the summary and cancel if the scope is incorrect.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Bulk action executes without confirmation prompt | LLM skipped the confirmation step | Review the system prompt in `src/api/chat.rs` to ensure confirmation instruction is present and clear; consider adding a code-level pre-execution check |
| Tool result shows high `failed` count | LLM hallucinated or used stale message IDs | Check if messages have been re-synced or deleted between the list step and the bulk_update step; IDs must match current `messages` table entries |
| POST /api/ai/chat returns error for bulk request | `bulk_update_emails` called with > 500 message IDs | LLM should have been informed of the cap in the tool definition; verify tool schema in `src/ai/tools.rs` includes the 500-cap constraint |
| `move_to_category` sets unrecognized category value | LLM supplied a non-standard category string | Add validation of allowed category values in the `bulk_update_emails` tool handler |
| BulkActionCard not rendering | ChatPanel not receiving action proposal structure from AI | Check the LLM response format in chat.rs — BulkActionCard renders only when the response contains an action proposal structure |
| AI does not attempt bulk operations at all | Tool not registered in the active tool list | Verify `bulk_update_emails` is included in the tool list passed to the provider in `src/ai/tools.rs` |

## Related Links

- Backend: `src/ai/tools.rs` (`bulk_update_emails` definition), `src/api/chat.rs` (agentic loop)
- Frontend: `web/src/components/BulkActionCard.svelte`
- Related: FH-009 (AI Chat — agentic loop, ACTION_PROPOSAL pattern), FH-005 (Inbox Management — batch update logic reused), FH-011 (V11 AI-Scalable Chat — `list_emails` and `search_emails` tools that feed bulk ops)
