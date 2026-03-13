# GC-534: Subscriptions List Empty State

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, empty-state, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No subscriptions have been discovered (clean state)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. List subscriptions when none exist
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array `[]`

3. Get stats when no subscriptions exist
   - **Target**: `GET http://localhost:3030/api/subscriptions/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with zero-count stats (e.g., `{"total": 0, "active": 0, "inactive": 0}`)

## Success Criteria
- [ ] GET /api/subscriptions returns 200 with empty array
- [ ] GET /api/subscriptions/stats returns 200 with zero counts
- [ ] Responses are valid JSON

## Failure Criteria
- Returns non-200 status code
- Returns null instead of empty array/zero stats
- Returns 500 error
