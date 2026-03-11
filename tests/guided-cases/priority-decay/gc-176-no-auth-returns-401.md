# GC-176: No Auth Token on Config Endpoint Returns 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, security, auth, 401, unauthorized
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- No session token (intentionally omitted)

## Steps
1. GET config without session token
   - **Target**: GET /api/config/ai
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized
2. PUT config without session token
   - **Target**: PUT /api/config/ai
   - **Input**: No `X-Session-Token` header, Header `Content-Type: application/json`, Body `{"decay_enabled": false}`
   - **Expected**: 401 Unauthorized
3. GET config with an invalid session token
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: invalid-token-value-99999`
   - **Expected**: 401 Unauthorized
4. PUT config with an invalid session token
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: invalid-token-value-99999`, Header `Content-Type: application/json`, Body `{"decay_enabled": false}`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] GET without token returns 401
- [ ] PUT without token returns 401
- [ ] GET with invalid token returns 401
- [ ] PUT with invalid token returns 401
- [ ] Config is not modified by any of the unauthenticated PUT attempts

## Failure Criteria
- Any unauthenticated request returns 200, 403, or 404 instead of 401
- Config value changes as a result of the unauthenticated PUT
- Server error (500)
