# GC-543: Analytics Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. GET overview with invalid token
   - **Target**: `GET http://localhost:3030/api/analytics/overview`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

2. GET hourly-distribution with invalid token
   - **Target**: `GET http://localhost:3030/api/analytics/hourly-distribution`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

3. GET response-times with invalid token
   - **Target**: `GET http://localhost:3030/api/analytics/response-times`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200 with invalid token
- Response contains analytics data
