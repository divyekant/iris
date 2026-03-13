# GC-345: Confirm with invalid action_id returns error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: compose, chat, confirm, invalid-id, 404, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- A valid session ID and a random UUID that does not correspond to any chat message — source: inline

## Steps

1. Attempt to confirm with a completely fabricated message_id
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc345-session", "message_id": "00000000-0000-0000-0000-000000000000"}'
     ```
   - **Expected**: HTTP 404 Not Found (the `query_row` in `confirm_action` maps a missing row to `StatusCode::NOT_FOUND`).

2. Attempt to confirm with a valid-format UUID for a message that exists but belongs to a different session
   - **Setup**: Create a real session and send a compose message:
     ```bash
     REAL_MSG_ID=$(curl -s -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc345-real-session", "message": "Draft an email to eve@example.com about the weekly sync"}' \
       | jq -r '.message.id')
     ```
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm` using `message_id` from real session but wrong `session_id`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d "{\"session_id\": \"gc345-wrong-session\", \"message_id\": \"$REAL_MSG_ID\"}"
     ```
   - **Expected**: HTTP 404 Not Found (the SQL query `WHERE id = ?1 AND session_id = ?2` will find no row because the session_id does not match).

3. Attempt to confirm a chat message that has no proposed action (a user message, not an assistant message)
   - **Setup**: Use the stored user message ID from the session:
     ```bash
     USER_MSG_ID=$(curl -s "http://localhost:3000/api/ai/chat/gc345-real-session" \
       -H "X-Session-Token: $TOKEN" | jq -r '[.[] | select(.role == "user")][0].id')
     ```
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d "{\"session_id\": \"gc345-real-session\", \"message_id\": \"$USER_MSG_ID\"}"
     ```
   - **Expected**: HTTP 400 Bad Request (the `proposed_action` JSON is null, which maps to `StatusCode::BAD_REQUEST` in `confirm_action`).

## Success Criteria
- [ ] Confirming with a non-existent `message_id` returns 404
- [ ] Confirming a valid message_id against the wrong session_id returns 404
- [ ] Confirming a user-role message (no proposed_action) returns 400
- [ ] No draft is created in any of the three failure scenarios

## Failure Criteria
- Server returns 200 or `executed: true` for any of the invalid cases
- Server returns 500 (unhandled panic) instead of 404 or 400
- A phantom draft is created despite the invalid confirm
