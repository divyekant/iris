# gc-mute-009: Mute Without Session Token (401 Unauthorized)

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: auth, security, 401, unauthorized, session-token, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- NO session token obtained (deliberately omitting authentication)

### Data
- Thread ID: `thread-test-mute-009` (source: inline)

## Steps
1. Attempt to mute without X-Session-Token header
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-009/mute`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. Attempt to check mute status without X-Session-Token header
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-009/mute`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

3. Attempt to unmute without X-Session-Token header
   - **Target**: `DELETE http://localhost:3030/api/threads/thread-test-mute-009/mute`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

4. Attempt to list muted threads without X-Session-Token header
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

5. Attempt to mute with invalid/forged session token
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-009/mute`
   - **Input**: Header `X-Session-Token: invalid-forged-token-abc123`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] PUT /api/threads/{id}/mute without token returns 401
- [ ] GET /api/threads/{id}/mute without token returns 401
- [ ] DELETE /api/threads/{id}/mute without token returns 401
- [ ] GET /api/muted-threads without token returns 401
- [ ] PUT with invalid/forged token returns 401
- [ ] No thread is muted as a result of any unauthenticated request

## Failure Criteria
- Any endpoint returns 200 OK without authentication
- Any endpoint returns 403 Forbidden instead of 401 (wrong auth error)
- Any endpoint returns 500 Internal Server Error
- Thread is muted despite failed authentication
