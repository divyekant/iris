# GC-537: Update Subscription Status and Verify

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, update-status, PUT, verify
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one subscription exists (run scan first if needed)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Scan for subscriptions
   - **Target**: `POST http://localhost:3030/api/subscriptions/scan`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK

3. List subscriptions and pick one
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least one subscription, note `id` and current `status`

4. Update subscription status
   - **Target**: `PUT http://localhost:3030/api/subscriptions/{id}/status`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"status": "inactive"}`
   - **Expected**: 200 OK with updated subscription showing `status` = `inactive`

5. Verify status change via detail endpoint
   - **Target**: `GET http://localhost:3030/api/subscriptions/{id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `status` = `inactive`

6. Verify stats reflect the change
   - **Target**: `GET http://localhost:3030/api/subscriptions/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with stats updated accordingly

## Success Criteria
- [ ] PUT returns 200 with updated subscription
- [ ] Status changed to "inactive"
- [ ] GET detail confirms status change
- [ ] Stats reflect the updated counts

## Failure Criteria
- PUT returns non-200
- Status not updated in subsequent GET
- Stats don't reflect the change
