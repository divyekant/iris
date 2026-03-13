# GC-533: Subscription Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. GET list subscriptions with invalid token
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

2. PUT update status with invalid token
   - **Target**: `PUT http://localhost:3030/api/subscriptions/some-id/status`
   - **Input**: Header `X-Session-Token: invalid-token-12345`, Header `Content-Type: application/json`, Body `{"status": "inactive"}`
   - **Expected**: 401 Unauthorized

3. GET stats with invalid token
   - **Target**: `GET http://localhost:3030/api/subscriptions/stats`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200 with invalid token
- Response contains subscription data
