# GC-290: Bulk mark_read via chat — mark all unread newsletters as read

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: bulk-chat-ops
- **Tags**: chat, bulk, batch-operations, mark-read
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap
- AI provider configured and available

### Data
- At least 2 unread emails categorized as newsletters or from a newsletter sender present (source: synced inbox or seed data)
- Fresh chat session ID

## Steps

1. Send a bulk mark-read request via AI Chat
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc290-session", "message": "Mark all my unread newsletter emails as read"}'
     ```
   - **Expected**: 200 OK, `message.proposed_action` is non-null with `action: "mark_read"` and a non-empty `message_ids` array; `message.content` describes the matching emails

2. Confirm the proposed action
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc290-session", "message_id": "<message_id_from_step_1>"}'
     ```
   - **Expected**: 200 OK, response body `{ "executed": true, "updated": N }` where N >= 1

3. Verify read state changed in the database
   - **Target**: `GET http://localhost:3000/api/messages?is_read=false`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/messages?is_read=false" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: Emails that were previously unread newsletters no longer appear in the unread filter results; their `is_read` field is now `1`

## Success Criteria
- [ ] Chat response status 200
- [ ] `message.proposed_action.action` equals `"mark_read"`
- [ ] `message.proposed_action.message_ids` is a non-empty array
- [ ] Confirm returns `{ "executed": true, "updated": N }` where N >= 1
- [ ] Affected messages have `is_read = 1` in the database after confirmation

## Failure Criteria
- `proposed_action` is null (AI didn't propose the action)
- `proposed_action.action` is not `"mark_read"`
- Confirm returns `executed: false`
- Database records still show `is_read = 0` after confirmation
