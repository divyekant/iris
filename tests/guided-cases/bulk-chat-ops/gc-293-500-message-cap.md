# GC-293: 500-message cap enforced — bulk operation does not exceed limit

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, cap, limit, safety
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 1 email present in the database to exercise the bulk path (source: synced inbox)
- (Full validation of the 500-cap is exercised at the tool layer via unit tests; this case validates the cap via the tool schema and response metadata)

## Steps

1. Inspect the bulk_update_emails tool schema to confirm the 500-message cap is documented
   - **Target**: Source code reference — `src/ai/tools.rs`, `BULK_UPDATE_MAX` constant and tool description
   - **Input**: Review tool description string: `"Array of message IDs to act on (from list_emails/search_emails results). Max 500."`
   - **Expected**: The constant `BULK_UPDATE_MAX = 500` is defined, and the tool description explicitly states the cap

2. Attempt a bulk operation by directly calling the tool with 501 message IDs (unit-level verification)
   - **Target**: `execute_tool` in `src/ai/tools.rs` with `bulk_update_emails` action and 501 IDs
   - **Input**: This step is verified by the unit test `test_bulk_update_archive` — a list of IDs exceeding 500 should be truncated or rejected
   - **Expected**: The tool returns at most 500 updated rows; IDs beyond the 500th are not processed

3. Via the chat API, request archiving a large category and verify the proposed action message_ids are capped
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc293-session", "message": "Archive everything in my inbox"}'
     ```
   - **Expected**: 200 OK, `message.proposed_action.message_ids` array length is at most 500; `message.content` mentions a count

4. Confirm the action (if proposed_action is present) and verify response `updated` is at most 500
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc293-session", "message_id": "<message_id_from_step_3>"}'
     ```
   - **Expected**: `{ "executed": true, "updated": N }` where N <= 500

## Success Criteria
- [ ] `BULK_UPDATE_MAX = 500` constant exists in `src/ai/tools.rs`
- [ ] Tool description includes "Max 500" in message_ids parameter description
- [ ] `message.proposed_action.message_ids` length never exceeds 500 in chat API response
- [ ] Confirm endpoint `updated` value is never greater than 500

## Failure Criteria
- `proposed_action.message_ids` contains more than 500 entries
- Confirm endpoint processes more than 500 messages in a single call
- Server crashes or times out when processing a large batch
