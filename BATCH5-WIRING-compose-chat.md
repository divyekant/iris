# Feature #63: Email Composition via Chat — Wiring Notes

## Shared File Changes Needed

### `web/src/lib/api.ts` (NOT modified per constraint)

The `chatConfirm` return type should be extended to include `draft_id`:

```typescript
chatConfirm: (data: { session_id: string; message_id: string }) =>
  request<{ executed: boolean; updated: number; draft_id?: string }>('/api/ai/chat/confirm', {
    method: 'POST',
    body: JSON.stringify(data),
  }),
```

Currently the ChatPanel works around this by using `const res: any = await api.ai.chatConfirm(...)` to access the `draft_id` field. Once the wiring merge is done, update the type and remove the `any` cast.

### `src/api/mod.rs` (NOT modified per constraint)

No changes needed — no new API endpoints were added. The compose_email tool works through the existing chat and confirm_action endpoints.

### `src/lib.rs` (NOT modified per constraint)

No changes needed — no new routes were added.

## Files Created

- `src/ai/tools.rs` — Tool definitions, ComposeEmailArgs, ComposeEmailData, handle_compose_email(), execute_compose_email(), 15 unit tests
- `web/src/components/chat/ComposeCard.svelte` — Card component showing draft preview with Edit & Send / Discard buttons

## Files Modified

- `src/ai/mod.rs` — Added `pub mod tools;`
- `src/api/chat.rs` — Extended system prompt with Email Composition section + COMPOSE_PROPOSAL format; added `data: Option<serde_json::Value>` to ProposedAction; added `draft_id: Option<String>` to ConfirmActionResponse; added compose_email handler in confirm_action(); added parse_compose_proposal in parse_action_proposal(); 3 new tests
- `web/src/components/ChatPanel.svelte` — Imported ComposeCard; added handleComposeEdit() and handleComposeDiscard(); renders ComposeCard for compose_email proposals; updated confirmAction() to handle draft_id response

## Integration Flow

1. User says "write an email to alice@example.com about the project update"
2. LLM generates response with `COMPOSE_PROPOSAL:{...}` at the end
3. `parse_action_proposal()` extracts the compose data into a `ProposedAction` with `action: "compose_email"` and `data` containing the compose fields
4. ChatPanel renders the assistant message with a `ComposeCard` showing To, Subject, Body preview
5. User clicks "Edit & Send" → dispatches `open-compose` CustomEvent with pre-filled data
6. User clicks "Discard" → removes the proposal from the message
7. Alternative: User clicks "Confirm" on the message → calls confirm_action → saves draft to DB → returns draft_id → opens compose modal
