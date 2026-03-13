# GC-544: Analytics with Empty Inbox

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, empty-state, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No messages synced (clean state or empty account)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Get overview with no data
   - **Target**: `GET http://localhost:3030/api/analytics/overview`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with zero/empty metrics (not null or error)

3. Get volume with no data
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=7`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array or zero-count entries

4. Get top contacts with no data
   - **Target**: `GET http://localhost:3030/api/analytics/top-contacts`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty array `[]`

## Success Criteria
- [ ] All analytics endpoints return 200 with valid empty/zero data
- [ ] No NaN, null, or undefined values in metrics
- [ ] Responses are valid JSON

## Failure Criteria
- Any endpoint returns 500
- Metrics contain NaN or null values
- Endpoints return non-200 when data is empty
