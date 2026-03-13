# GC-291: Chat refuses to execute bulk operation without explicit user confirmation

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, confirmation, safety
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 1 email from any sender present (source: synced inbox or seed data)

## Steps

1. Send a bulk operation request via AI Chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc291-session", "message": "Archive all promotional emails"}'
     ```
   - **Expected**: 200 OK, `message.content` describes the proposed operation; `message.proposed_action` is non-null with `action: "archive"`. The AI does NOT immediately execute the action — it only proposes it.

2. Verify no emails were actually moved without confirmation
   - **Target**: `GET http://localhost:3000/api/messages?folder=Archive`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?folder=Archive" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: Archive count has not increased relative to baseline; promotional emails that would have been affected are still in their original folder (Inbox or Promotions category)

3. Attempt to call confirm with a fabricated (non-existent) message_id
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -w "\n%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc291-session", "message_id": "does-not-exist-id"}'
     ```
   - **Expected**: 404 Not Found — the confirm endpoint refuses to execute when the message_id does not match a stored proposed_action

4. Confirm using the real message_id from step 1 to verify the confirm endpoint is what triggers execution
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc291-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK, `{ "executed": true, "updated": N }` — execution only happens after explicit confirm call

## Success Criteria
- [ ] Initial chat response proposes the action but does not execute it (emails still in original folder after step 1)
- [ ] `proposed_action` is present in the initial chat response
- [ ] Fabricated message_id returns 404 from the confirm endpoint
- [ ] Only after calling confirm with the real message_id does execution occur

## Failure Criteria
- Emails are moved during the initial chat call (before confirmation)
- Fabricated message_id confirm attempt returns 200 with `executed: true`
- `proposed_action` is absent in the initial response
