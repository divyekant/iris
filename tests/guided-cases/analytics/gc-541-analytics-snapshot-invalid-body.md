# GC-541: Analytics Snapshot with Invalid Body

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, validation, snapshot, invalid-body
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Create snapshot with invalid JSON
   - **Target**: `POST http://localhost:3030/api/analytics/snapshot`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{invalid json}`
   - **Expected**: 400 Bad Request

3. Create snapshot with empty body
   - **Target**: `POST http://localhost:3030/api/analytics/snapshot`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body ``
   - **Expected**: 400 Bad Request or 200 OK (if no body required)

## Success Criteria
- [ ] Invalid JSON returns 400
- [ ] Server does not crash on malformed input

## Failure Criteria
- Invalid JSON returns 200
- Server returns 500 (unhandled parse error)
