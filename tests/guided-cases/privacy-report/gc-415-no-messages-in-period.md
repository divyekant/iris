# GC-415: Privacy report with no messages in period returns zeroed stats

## Metadata
- **Type**: edge
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
- A valid account with no messages received in the last 1 day (account has older messages but nothing recent, OR use a freshly created account with no messages)

## Steps
1. Fetch the privacy report with a very short window that covers no messages
   - **Target**: `GET /api/privacy/report?account_id=1&days=1`
   - **Input**: account_id = 1, days = 1 (assuming no messages arrived in the last 24 hours)
   - **Expected**: 200 OK with zeroed stats

2. Verify all numeric fields are correctly zeroed
   - **Target**: Response JSON inspection
   - **Input**: Check all numeric and percentage fields
   - **Expected**: `total_messages_scanned` = 0, `tracking_pixels_found` = 0, `link_trackers_found` = 0, `top_trackers` = [], `clean_messages_percentage` = 100 (or 0 if undefined for empty set), `trackers_blocked_percentage` = 0 (or 100 if undefined for empty set)

3. Verify comparison object handles no-data case gracefully
   - **Target**: Response JSON `comparison` field
   - **Input**: Inspect comparison sub-object
   - **Expected**: `comparison` is present and well-formed; previous period values are 0 or null — no division-by-zero error, no NaN, no null pointer in the response body

4. Fetch the tracker list for the same empty window
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=20`
   - **Input**: account_id = 1, limit = 20
   - **Expected**: 200 OK with an empty array `[]`

## Success Criteria
- [ ] Response status is 200 (not 404 or 500)
- [ ] `total_messages_scanned` is 0
- [ ] `tracking_pixels_found` is 0
- [ ] `link_trackers_found` is 0
- [ ] `top_trackers` is an empty array
- [ ] `clean_messages_percentage` is a valid number (not NaN, null, or undefined)
- [ ] `trackers_blocked_percentage` is a valid number (not NaN, null, or undefined)
- [ ] `comparison` is present and does not contain NaN or null for numeric fields
- [ ] Tracker list endpoint returns an empty array

## Failure Criteria
- 500 error (division by zero or null dereference in percentage calculation)
- `clean_messages_percentage` or `trackers_blocked_percentage` is NaN, Infinity, or null
- `top_trackers` is null instead of an empty array
- `comparison` contains NaN or causes a serialization error

## Notes
The key risk here is division-by-zero when computing percentages (e.g., `tracking_pixels_found / total_messages_scanned`). When `total_messages_scanned` = 0, the server must guard against this and return a sensible default (100 for clean_messages_percentage, 0 for trackers_blocked_percentage, or vice versa per business logic).
