# GC-083: Rapid Toggle — Idempotent Behavior

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, toggle, rapid, idempotent, edge, upsert
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one account exists (source: local-db, fetch via `GET /api/accounts` and use first element's `id`)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Fetch account list to get a valid account ID
   - **Target**: `GET http://localhost:3030/api/accounts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response is a JSON array with at least one account object

3. Disable notifications
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 200 OK, `{"enabled": false}`

4. Disable notifications again (idempotent repeat)
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 200 OK, `{"enabled": false}`

5. Enable notifications
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": true}`
   - **Expected**: 200 OK, `{"enabled": true}`

6. Enable notifications again (idempotent repeat)
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": true}`
   - **Expected**: 200 OK, `{"enabled": true}`

7. Rapid cycle: disable, enable, disable, enable, disable
   - **Target**: `PUT http://localhost:3030/api/accounts/{account_id}/notifications` (5 sequential calls)
   - **Input**: Bodies in order: `{"enabled": false}`, `{"enabled": true}`, `{"enabled": false}`, `{"enabled": true}`, `{"enabled": false}`
   - **Expected**: Each returns 200 OK with the matching `enabled` value; final state is `{"enabled": false}`

8. Verify final state
   - **Target**: `GET http://localhost:3030/api/accounts/{account_id}/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `{"enabled": false}`

## Success Criteria
- [ ] Duplicate disable calls both return 200 with `{"enabled": false}`
- [ ] Duplicate enable calls both return 200 with `{"enabled": true}`
- [ ] All 5 rapid-cycle calls return 200 with correct matching state
- [ ] Final GET confirms last-write-wins (`{"enabled": false}`)
- [ ] Only one config row exists for key `notifications_{account_id}` (no duplicates from rapid UPSERT)

## Failure Criteria
- Any PUT returns non-200 during rapid toggling
- Duplicate state produces a different response on repeat
- Multiple config rows created for the same key (UPSERT violation)
- Final state does not reflect the last PUT
