# GC-513: Health Report Endpoints With Invalid Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, auth, invalid-token, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- Invalid token: `invalid-token-12345`

## Steps
1. POST generate report with invalid token
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

2. GET list reports with invalid token
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

3. DELETE report with invalid token
   - **Target**: `DELETE http://localhost:3030/api/health-reports/some-id`
   - **Input**: Header `X-Session-Token: invalid-token-12345`
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All endpoints return 401 with invalid session token
- [ ] No data leaked in any response

## Failure Criteria
- Any endpoint returns 200/201 with invalid token
- Response contains report data
