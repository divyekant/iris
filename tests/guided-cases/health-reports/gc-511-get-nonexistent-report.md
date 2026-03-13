# GC-511: Get Nonexistent Health Report

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, validation, not-found, 404
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Non-existent report ID: `nonexistent-report-999`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET nonexistent report
   - **Target**: `GET http://localhost:3030/api/health-reports/nonexistent-report-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. DELETE nonexistent report
   - **Target**: `DELETE http://localhost:3030/api/health-reports/nonexistent-report-999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] GET returns 404 for nonexistent report ID
- [ ] DELETE returns 404 for nonexistent report ID

## Failure Criteria
- Returns 200 with empty/null data
- Returns 500 Internal Server Error
