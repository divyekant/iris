# GC-532: Subscription Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. GET list subscriptions without session token
   - **Target**: `GET http://localhost:3030/api/subscriptions`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

2. POST scan subscriptions without session token
   - **Target**: `POST http://localhost:3030/api/subscriptions/scan`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. GET subscription detail without session token
   - **Target**: `GET http://localhost:3030/api/subscriptions/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

4. POST bulk-action without session token
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `Content-Type: application/json`, Body `{"ids": ["x"], "action": "unsubscribe"}`
   - **Expected**: 401 Unauthorized

5. GET stats without session token
   - **Target**: `GET http://localhost:3030/api/subscriptions/stats`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All five endpoints return 401 without X-Session-Token
- [ ] No subscription data leaked in response bodies

## Failure Criteria
- Any endpoint returns 200 without authentication
- Response body contains subscription data
