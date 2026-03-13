# GC-295: Cancel bulk operation — user declines confirmation, emails are not modified

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, cancel, confirmation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 2 emails from a recognizable sender present in the database (source: synced inbox or seed data)

## Steps

1. Send a bulk operation request via AI Chat to get a proposed action
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc295-session", "message": "Archive all emails from GitHub notifications"}'
     ```
   - **Expected**: 200 OK, `message.proposed_action` is non-null; note the `message.id` value

2. Record the current email count in the Inbox (baseline)
   - **Target**: `GET http://localhost:3000/api/messages?folder=Inbox`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?folder=Inbox" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: Returns a list; record the total count as baseline

3. Cancel by sending a follow-up message saying "never mind" (do NOT call confirm)
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc295-session", "message": "Never mind, cancel that"}'
     ```
   - **Expected**: 200 OK, `message.content` acknowledges the cancellation (e.g., "Alright, I won't archive those emails"); `message.proposed_action` is null (no new action proposed)

4. Verify inbox count is unchanged
   - **Target**: `GET http://localhost:3000/api/messages?folder=Inbox`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?folder=Inbox" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: Email count matches the baseline from step 2; no emails were moved to Archive

5. Verify the original proposed action's message_id does not execute if confirm is called now
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -w "\n%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc295-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: The confirm endpoint still processes the stored proposed_action from step 1 if called (the message is still in the DB). This step documents that cancellation is a user-side concern — calling confirm with the original message_id after cancelling is NOT blocked at the API level. The test verifies that the application layer (frontend) is responsible for not presenting the Confirm button after a cancel message, not the backend.

## Success Criteria
- [ ] Initial chat response provides a proposed_action
- [ ] Cancel message is acknowledged without executing the action
- [ ] Inbox email count is identical before and after the cancel message
- [ ] No Archive operation is triggered by the cancel message
- [ ] Second chat message has `proposed_action: null`

## Failure Criteria
- Emails are moved to Archive during or after the cancel message
- Second chat response also proposes an action
- Server returns non-200 for either chat call
