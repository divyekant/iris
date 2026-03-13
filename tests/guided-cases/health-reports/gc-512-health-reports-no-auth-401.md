# GC-512: Health Report Endpoints Without Auth Return 401

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, auth, 401, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030

### Data
- None

## Steps
1. POST generate report without session token
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

2. GET list reports without session token
   - **Target**: `GET http://localhost:3030/api/health-reports`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

3. GET single report without session token
   - **Target**: `GET http://localhost:3030/api/health-reports/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

4. DELETE report without session token
   - **Target**: `DELETE http://localhost:3030/api/health-reports/some-id`
   - **Input**: (no auth headers)
   - **Expected**: 401 Unauthorized

## Success Criteria
- [ ] All four endpoints return 401 without X-Session-Token
- [ ] No report data leaked in response bodies

## Failure Criteria
- Any endpoint returns 200/201 without authentication
- Response body contains report data
