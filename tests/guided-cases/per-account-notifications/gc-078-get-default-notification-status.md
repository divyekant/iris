# GC-078: Get Default Notification Status (Enabled)

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, get, default, happy-path
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one account exists (source: local-db, fetch via `GET /api/accounts` and use first element's `id`)
- No row in `config` table with key `notifications_{account_id}` for the chosen account (source: local-db, verify absence)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Fetch account list to get a valid account ID
   - **Target**: `GET http://localhost:3030/api/accounts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response is a JSON array with at least one account object containing an `id` field

3. Get notification preference for the account
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response body is `{"enabled": true}`

## Success Criteria
- [ ] GET returns 200 status code
- [ ] Response body contains `"enabled": true`
- [ ] No config row exists in DB for this account — the default is applied server-side

## Failure Criteria
- GET returns non-200 status code
- Response body contains `"enabled": false` when no config row exists
- Response body is missing the `enabled` field
