# GC-535: Get Nonexistent Subscription Returns 404

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, not-found, 404, GET
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

2. GET nonexistent subscription
   - **Target**: `GET http://localhost:3030/api/subscriptions/nonexistent-sub-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET returns 404 for nonexistent subscription ID

## Failure Criteria
- Returns 200 with null/empty data
- Returns 500 Internal Server Error
