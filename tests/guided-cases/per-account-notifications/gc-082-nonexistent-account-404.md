# GC-082: Get/Set Notifications for Non-existent Account (404)

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, 404, nonexistent-account, boundary, get, put
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A fabricated account ID that does not exist: `00000000-0000-0000-0000-000000000000` (source: inline)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET notification preference for non-existent account
   - **Target**: `GET http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000000/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. PUT notification preference for non-existent account
   - **Target**: `PUT http://localhost:3030/api/accounts/00000000-0000-0000-0000-000000000000/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET returns 404 for non-existent account
- [ ] PUT returns 404 for non-existent account
- [ ] No config row is created in the database for the non-existent account
- [ ] Response body includes an error message (not an empty body)

## Failure Criteria
- Either endpoint returns 200 OK for a non-existent account
- A config row is silently created for a non-existent account ID
- Server returns 500 Internal Server Error instead of 404
