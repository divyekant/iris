# GC-347: No auth on chat returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: compose-via-chat
- **Tags**: auth, security, 401, unauthorized, session-token, compose
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- NO valid session token (deliberately omitted or forged)

### Data
- No special data required — all requests in this test use no or invalid auth

## Steps

1. Attempt to send a compose chat message without X-Session-Token header
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc347-session", "message": "Draft an email to grace@example.com"}'
     ```
   - **Expected**: HTTP 401 Unauthorized.

2. Attempt to send a compose chat message with a forged/invalid token
   - **Target**: `POST http://localhost:3000/api/ai/chat`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat \
       -H "X-Session-Token: forged-token-abc123xyz" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc347-session", "message": "Draft an email to grace@example.com"}'
     ```
   - **Expected**: HTTP 401 Unauthorized.

3. Attempt to call the confirm endpoint without X-Session-Token header
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc347-session", "message_id": "00000000-0000-0000-0000-000000000001"}'
     ```
   - **Expected**: HTTP 401 Unauthorized.

4. Attempt to call the confirm endpoint with a forged token
   - **Target**: `POST http://localhost:3000/api/ai/chat/confirm`
   - **Input**:
     ```bash
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/chat/confirm \
       -H "X-Session-Token: definitely-not-real" \
       -H "Content-Type: application/json" \
       -d '{"session_id": "gc347-session", "message_id": "00000000-0000-0000-0000-000000000001"}'
     ```
   - **Expected**: HTTP 401 Unauthorized.

5. Verify no messages or drafts were created by any unauthenticated request
   - **Target**: Internal state check
   - **Method**: Using a valid token, check that no chat messages were stored for session `gc347-session` and no new drafts exist:
     ```bash
     curl -s "http://localhost:3000/api/ai/chat/gc347-session" \
       -H "X-Session-Token: $TOKEN" | jq 'length'
     ```
   - **Expected**: Empty array or 0 (no messages stored from unauthenticated requests).

## Success Criteria
- [ ] `POST /api/ai/chat` without token returns 401
- [ ] `POST /api/ai/chat` with invalid token returns 401
- [ ] `POST /api/ai/chat/confirm` without token returns 401
- [ ] `POST /api/ai/chat/confirm` with invalid token returns 401
- [ ] No chat messages stored from unauthenticated requests
- [ ] No drafts created from unauthenticated requests

## Failure Criteria
- Any endpoint returns 200 without a valid session token
- Any endpoint returns 403 instead of 401 (wrong auth error code)
- Any endpoint returns 500 (unauthenticated request causes a panic)
- A chat message or draft is persisted despite failed authentication
