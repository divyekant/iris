# GC-079: Disable Notifications for an Account

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, put, disable, happy-path
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one account exists (source: local-db, fetch via `GET /api/accounts` and use first element's `id`)
- Account currently has notifications enabled (default state, or explicitly set)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Fetch account list to get a valid account ID
   - **Target**: `GET http://localhost:3030/api/accounts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response is a JSON array with at least one account object

3. Disable notifications for the account
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 200 OK, response body is `{"enabled": false}`

4. Verify the change persists by reading it back
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response body is `{"enabled": false}`

## Success Criteria
- [ ] PUT returns 200 status code
- [ ] PUT response body contains `"enabled": false`
- [ ] Subsequent GET returns `"enabled": false`
- [ ] Config table contains row with key `notifications_{account_id}` and value `"disabled"`

## Failure Criteria
- PUT returns non-200 status code
- GET after PUT still returns `"enabled": true`
- Config row not written to database
