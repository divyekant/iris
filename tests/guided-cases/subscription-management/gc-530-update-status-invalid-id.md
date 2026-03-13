# GC-530: Update Subscription Status with Invalid ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, validation, not-found, PUT
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent subscription ID: `nonexistent-sub-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Update status of nonexistent subscription
   - **Target**: `PUT http://localhost:3030/api/subscriptions/nonexistent-sub-999/status`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"status": "inactive"}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] PUT returns 404 for nonexistent subscription ID

## Failure Criteria
- Returns 200 or creates a new subscription
- Returns 500 Internal Server Error
