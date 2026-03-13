# GC-538: Bulk Action Unsubscribe Multiple Subscriptions

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, bulk-action, unsubscribe, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least two subscriptions exist (run scan first if needed)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Scan and list subscriptions
   - **Target**: `POST http://localhost:3030/api/subscriptions/scan`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK

3. Get subscription IDs
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least 2 subscriptions, note their `id` values

4. Bulk unsubscribe
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"ids": ["{id_1}", "{id_2}"], "action": "unsubscribe"}`
   - **Expected**: 200 OK with result indicating number of affected subscriptions (e.g., `{"updated": 2}`)

5. Verify subscriptions updated
   - **Target**: `GET http://localhost:3030/api/subscriptions/{id_1}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with updated status reflecting unsubscription

## Success Criteria
- [ ] Bulk action returns 200 with updated count matching number of IDs
- [ ] Individual subscription statuses updated after bulk action
- [ ] Stats endpoint reflects the bulk change

## Failure Criteria
- Bulk action returns error
- Updated count does not match expected
- Subscriptions not actually updated
