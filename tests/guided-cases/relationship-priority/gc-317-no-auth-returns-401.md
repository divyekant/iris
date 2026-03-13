# GC-317: No Auth Token Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, auth, 401, security, no-token
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- No session token is used in the requests

### Data
- None required (source: inline)

## Steps

1. Request relationship score computation without auth
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: No `X-Session-Token` header; no body
   - **Expected**: 401 Unauthorized

2. Request relationship score for a contact without auth
   - **Target**: `GET http://localhost:3000/api/contacts/alice@example.com/relationship`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

3. Request prioritized messages without auth
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id=1&folder=INBOX`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

4. Repeat each request with a forged token
   - **Target**: All three endpoints above
   - **Input**: Header `X-Session-Token: invalid-token-abc123`
   - **Expected**: 401 Unauthorized for all three

5. Verify 401 responses do not leak data
   - **Target**: Response bodies from steps 1–4
   - **Input**: Inspect body content
   - **Expected**: No relationship score data, no message data, no internal structure revealed

## Success Criteria
- [ ] All unauthenticated requests return 401
- [ ] All requests with forged tokens return 401
- [ ] No relationship score or message data is returned in any 401 response
- [ ] No 500 error triggered by missing token

## Failure Criteria
- Any endpoint returns 200 without a valid session token
- Any endpoint returns 403 instead of 401 (wrong semantics for missing auth)
- Data leaks in the 401 response body
- Server panics or returns 500 on missing auth header

## Notes
All three relationship-priority endpoints share the same session auth middleware. Testing all three in a single case ensures the middleware is applied uniformly and not accidentally omitted from any route registration.
