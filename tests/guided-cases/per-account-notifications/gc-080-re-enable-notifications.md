# GC-080: Re-enable Notifications for an Account

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, put, enable, re-enable, happy-path, upsert
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one account exists (source: local-db, fetch via `GET /api/accounts` and use first element's `id`)
- Notifications previously disabled for that account (source: local-db, a config row with key `notifications_{account_id}` and value `"disabled"` must exist — run the disable step first if needed)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Fetch account list to get a valid account ID
   - **Target**: `GET http://localhost:3030/api/accounts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response is a JSON array with at least one account object

3. Confirm notifications are currently disabled
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response body is `{"enabled": false}`

4. Re-enable notifications
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": true}`
   - **Expected**: 200 OK, response body is `{"enabled": true}`

5. Verify the change persists
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response body is `{"enabled": true}`

## Success Criteria
- [ ] Step 3 confirms notifications were disabled (precondition verified)
- [ ] PUT returns 200 status code with `"enabled": true`
- [ ] Subsequent GET returns `"enabled": true`
- [ ] Config table row with key `notifications_{account_id}` now has value `"enabled"` (UPSERT updated existing row)

## Failure Criteria
- PUT returns non-200 status code
- GET after PUT still returns `"enabled": false`
- A duplicate config row is created instead of updating the existing one (UPSERT failure)
