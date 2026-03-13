# GC-510: Generate Health Report with Empty Inbox

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, validation, empty-inbox, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No messages in the inbox (or account with empty inbox)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Generate report when no data available
   - **Target**: `POST http://localhost:3030/api/health-reports/generate`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with report containing zero/baseline metrics, or 400/422 indicating insufficient data

## Success Criteria
- [ ] Endpoint handles empty inbox gracefully (no 500 error)
- [ ] Returns either a valid baseline report or clear error message

## Failure Criteria
- Returns 500 Internal Server Error
- Crashes or hangs during generation
- Returns report with NaN or null metric values
