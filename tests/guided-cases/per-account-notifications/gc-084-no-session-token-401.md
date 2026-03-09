# GC-084: Set Notifications Without Session Token (401)

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, auth, 401, unauthorized, session-token, security
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- A plausible account ID (source: inline, use `00000000-0000-0000-0000-000000000001` — does not need to exist since auth check should happen first)

## Steps
1. GET notifications without session token
   - **Target**: `GET http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000001/notifications`
   - **Input**: No `X-Session-Token` header
   - **Expected**: 401 Unauthorized

2. PUT notifications without session token
   - **Target**: `PUT http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000001/notifications`
   - **Input**: No `X-Session-Token` header, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 401 Unauthorized

3. GET notifications with invalid session token
   - **Target**: `GET http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000001/notifications`
   - **Input**: Header `X-Session-Token: invalid-token-value-12345`
   - **Expected**: 401 Unauthorized

4. PUT notifications with invalid session token
   - **Target**: `PUT http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000001/notifications`
   - **Input**: Header `X-Session-Token: invalid-token-value-12345`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] GET without token returns 401
- [ ] PUT without token returns 401
- [ ] GET with invalid token returns 401
- [ ] PUT with invalid token returns 401
- [ ] No response body leaks account existence information
- [ ] Auth check occurs before account-existence check (401 before 404)

## Failure Criteria
- Any request without valid auth returns 200, 403, or 404 instead of 401
- Response body reveals whether the account ID exists
- Server returns 500 Internal Server Error
