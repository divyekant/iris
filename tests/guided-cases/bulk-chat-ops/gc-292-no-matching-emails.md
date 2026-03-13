# GC-292: Bulk operation with no matching emails — graceful zero-result handling

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, empty-results, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- No emails from `noreply@nonexistentsender-xyz-test.example.com` in the database (guaranteed by using a fabricated sender)

## Steps

1. Send a bulk operation request targeting a sender that does not exist
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc292-session", "message": "Archive all emails from noreply@nonexistentsender-xyz-test.example.com"}'
     ```
   - **Expected**: 200 OK, `message.content` informs the user that no matching emails were found (e.g., "I couldn't find any emails from that sender" or similar); `message.proposed_action` is null or absent (no action to confirm when no emails match)

2. Verify no proposed_action is set
   - **Target**: Response JSON `message.proposed_action`
   - **Expected**: Field is null or absent — there is nothing to confirm

3. Attempt to call confirm endpoint without a valid proposed_action message
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -w "\n%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc292-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: Either 400 Bad Request (no proposed_action on the message) or `{ "executed": false, "updated": 0 }` — the system does not error out but correctly reports zero updates

## Success Criteria
- [ ] Chat response status 200
- [ ] `message.content` communicates that no matching emails exist
- [ ] `message.proposed_action` is null or absent
- [ ] Confirm endpoint does not crash; returns 400 or `{ "executed": false, "updated": 0 }`
- [ ] No emails are modified in the database

## Failure Criteria
- Server returns non-200 from the chat endpoint
- `proposed_action` is present with an empty `message_ids` array and confirm processes silently
- Confirm endpoint returns 500 when no proposed_action exists on the message
