# GC-576: Cached Phishing Report Retrieved Without Re-Scanning

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, cached-report, GET, idempotency
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message that has been previously scanned (via POST /api/security/phishing-scan/{message_id})

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Retrieve the cached phishing report
   - **Target**: `GET http://localhost:3030/api/security/phishing-report/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response matches the original scan result, with `cached: true` or `scanned_at` timestamp

3. Retrieve report for a message that has never been scanned
   - **Target**: `GET http://localhost:3030/api/security/phishing-report/{unscanned_message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found (no cached report exists)

## Success Criteria
- [ ] GET returns 200 for previously scanned message
- [ ] Response data consistent with original scan
- [ ] GET returns 404 for never-scanned message
- [ ] Cached report includes `scanned_at` timestamp

## Failure Criteria
- GET returns 200 for unscanned message
- Cached data inconsistent with original scan
- 5xx error on valid message ID
