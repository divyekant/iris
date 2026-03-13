# GC-413: Invalid account_id returns empty report or 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: privacy-report
- **Tags**: privacy, trackers, report, scanning
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No account with ID 99999 exists in the database

## Steps
1. Fetch the privacy report for a non-existent account
   - **Target**: `GET /api/privacy/report?account_id=99999&days=30`
   - **Input**: account_id = 99999 (does not exist), days = 30
   - **Expected**: Either 200 OK with all zeroed stats (empty report) OR 404 Not Found with a descriptive error message

2. Verify the response does not leak other accounts' data
   - **Target**: Response JSON inspection
   - **Input**: Check all numeric fields
   - **Expected**: If 200, `total_messages_scanned` = 0, `tracking_pixels_found` = 0, `link_trackers_found` = 0, `clean_messages_percentage` = 100 or 0, `top_trackers` = []

3. Fetch the tracker list for the same invalid account
   - **Target**: `GET /api/privacy/trackers?account_id=99999&limit=20`
   - **Input**: account_id = 99999, limit = 20
   - **Expected**: Either 200 OK with empty array OR 404 Not Found — no data from other accounts

4. Verify a non-integer account_id is rejected
   - **Target**: `GET /api/privacy/report?account_id=abc&days=30`
   - **Input**: account_id = "abc" (non-numeric string)
   - **Expected**: 400 Bad Request with a validation error message

## Success Criteria
- [ ] Non-existent numeric account_id returns 200 with zeroed stats OR 404
- [ ] If 200, `top_trackers` is an empty array
- [ ] If 200, no data from other accounts is present
- [ ] Non-integer account_id returns 400
- [ ] No 500 errors from any of these requests

## Failure Criteria
- 500 Internal Server Error for any input
- Data from other accounts returned for account_id=99999
- Non-integer account_id returns 200 or 500 instead of 400
- Error responses expose internal stack traces or SQL details

## Notes
Tests both unknown numeric IDs (which should return empty data or 404) and malformed non-integer IDs (which should fail validation at the parameter-parsing layer before hitting the database).
