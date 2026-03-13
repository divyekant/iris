# GC-357: No Auth Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, auth, 401, unauthenticated, session-token, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- No session token (intentionally omitted)
- A valid-looking message ID string (actual DB presence not required)

## Steps
1. Attempt to trigger analysis without a session token
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: No `X-Session-Token` header; body `{ "message_id": "msg-123" }`
   - **Expected**: 401 Unauthorized

2. Attempt to retrieve cached result without a session token
   - **Target**: GET http://127.0.0.1:3000/api/messages/msg-123/social-engineering
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

3. Attempt POST with an invalid/expired session token
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: invalid-token-xyz`, body `{ "message_id": "msg-123" }`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] POST without token returns 401
- [ ] GET without token returns 401
- [ ] POST with invalid token returns 401
- [ ] None of the unauthenticated requests return 200 or trigger AI analysis
- [ ] No sensitive data is leaked in error responses

## Failure Criteria
- Any unauthenticated request returns 200
- Any unauthenticated request returns 403 instead of 401
- Server returns 500 for missing/invalid auth header
- Response body contains analysis data for an unauthenticated caller
