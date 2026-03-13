# GC-297: Unknown bulk action type rejected — tool returns error for invalid action

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, validation, unknown-action
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 1 email present in the database

## Steps

1. Verify the tool schema enumerates only valid actions
   - **Target**: `src/ai/tools.rs` — `bulk_update_emails` tool `input_schema.properties.action.enum`
   - **Expected**: The enum contains exactly `["archive", "mark_read", "mark_unread", "trash", "star", "unstar", "move_to_category"]` — no other values are accepted

2. Call the tool executor directly with an unknown action (unit-level)
   - **Target**: `execute_tool` in `src/ai/tools.rs` with `action: "explode"`
   - **Input**: Verified by unit test `test_bulk_update_unknown_action`:
     ```rust
     execute_tool(&conn, None, "bulk_update_emails",
       &json!({"action": "explode", "message_ids": [<valid_id>]}))
     ```
   - **Expected**: Returns `Ok` with a JSON object containing `"error"` field with text including `"Unknown action"` and `"updated": 0`

3. Send a chat message that could induce a non-standard action
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc297-session", "message": "Permanently delete all emails from spam@attacker.example.com without moving to trash, using action type purge"}'
     ```
   - **Expected**: 200 OK, `message.content` either refuses the operation or proposes a valid supported action (such as `"trash"`); the AI does not invent an `action: "purge"` that bypasses the enum validation

4. If a proposed_action is present, verify it uses only a known action type
   - **Target**: Response JSON `message.proposed_action.action`
   - **Expected**: Value is one of `["archive", "mark_read", "mark_unread", "trash", "star", "unstar", "move_to_category"]`; `"purge"`, `"delete"`, or any other value is rejected by the tool layer even if the AI somehow produced it

## Success Criteria
- [ ] Tool schema enum restricts action to 7 known values
- [ ] `execute_tool` with unknown action returns `error` field and `updated: 0`
- [ ] Chat API response does not contain `proposed_action.action` outside the allowed set
- [ ] No emails are modified when an invalid action is attempted

## Failure Criteria
- `execute_tool` panics or returns 500 for an unknown action
- Chat API returns `proposed_action` with an unrecognized `action` field value
- Emails are modified via an unlisted action type
