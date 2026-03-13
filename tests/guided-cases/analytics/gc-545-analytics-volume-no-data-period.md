# GC-545: Analytics Volume for Period with No Data

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, volume, no-data, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Some messages exist but none in the queried date range

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get volume for a very short recent window (likely no data)
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array or 400 (if days=0 invalid)

3. Get volume with days=1
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at most one data point (may have zero count)

## Success Criteria
- [ ] Endpoints handle boundary day values gracefully
- [ ] No 500 errors for edge-case period lengths
- [ ] Response format is consistent regardless of data volume

## Failure Criteria
- Returns 500 for boundary values
- Response format changes when no data exists for period
