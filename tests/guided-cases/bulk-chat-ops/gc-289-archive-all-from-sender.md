# GC-289: Happy path — archive all emails from LinkedIn after confirmation

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, archive, confirm
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 2 emails from a recognizable sender present in the database (e.g., `linkedin.com` domain) (source: synced inbox or seed data)
- Known session ID for a fresh chat session

## Steps

1. Send a bulk archive request via AI Chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc289-session", "message": "Archive all emails from LinkedIn"}'
     ```
   - **Expected**: 200 OK, response JSON contains `message.content` describing how many LinkedIn emails will be archived (e.g., "I found N emails from LinkedIn and will archive them"), and `message.proposed_action` is non-null with `action: "archive"` and a non-empty `message_ids` array

2. Capture session_id and message_id from the response
   - **Target**: Response JSON `message.session_id` and `message.id`
   - **Expected**: Both are non-empty strings

3. Confirm the proposed action
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc289-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK, response body `{ "executed": true, "updated": N }` where N >= 1

4. Verify emails were archived in the database
   - **Target**: `GET http://localhost:3000/api/messages?folder=Archive`
   - **Input**:
     ```bash
     curl -s http://localhost:3000/api/messages?folder=Archive \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: Previously LinkedIn-from emails appear in the Archive folder, no longer in Inbox

## Success Criteria
- [ ] Chat response status 200
- [ ] `message.proposed_action` is non-null with `action: "archive"`
- [ ] `message.proposed_action.message_ids` contains at least 1 ID
- [ ] `message.content` mentions a count of matching emails
- [ ] Confirm endpoint returns 200 with `{ "executed": true, "updated": N }` (N >= 1)
- [ ] Archived emails have `folder = 'Archive'` in the database

## Failure Criteria
- Chat endpoint returns non-200
- `proposed_action` is null (no confirmation step offered)
- Confirm endpoint returns `{ "executed": false, "updated": 0 }` when matching emails exist
- Database shows emails still in Inbox after confirmation
