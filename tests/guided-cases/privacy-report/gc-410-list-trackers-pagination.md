# GC-410: Happy path — list trackers with pagination

## Metadata
- **Type**: positive
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
- At least one synced account with messages containing tracker domains (source: prior sync or seed data)
- More than 5 tracker detections in the database to exercise meaningful pagination

## Steps
1. Fetch tracker list with default limit
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=20`
   - **Input**: account_id = 1, limit = 20
   - **Expected**: 200 OK with JSON array of tracker detections, at most 20 entries

2. Verify each tracker entry has the required fields
   - **Target**: Response JSON array inspection
   - **Input**: Inspect each element
   - **Expected**: Each entry contains `message_id` (string), `tracker_name` (string), `tracker_type` (string: "OpenPixel" or "LinkTrack"), `detected_at` (ISO 8601 timestamp string)

3. Fetch with a reduced limit to verify limit is respected
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=3`
   - **Input**: account_id = 1, limit = 3 (assuming > 3 tracker detections exist)
   - **Expected**: 200 OK with exactly 3 entries in the response array

4. Verify ordering is consistent
   - **Target**: Response JSON array from step 3
   - **Input**: Compare `detected_at` timestamps across entries
   - **Expected**: Entries are ordered by `detected_at` descending (most recent first) or in a stable deterministic order

## Success Criteria
- [ ] Response status is 200
- [ ] Response body is a JSON array
- [ ] Array length is ≤ the requested limit
- [ ] Each entry has `message_id`, `tracker_name`, `tracker_type`, and `detected_at` fields
- [ ] `tracker_type` values are only "OpenPixel" or "LinkTrack"
- [ ] `detected_at` is a valid ISO 8601 timestamp string
- [ ] Reducing the limit to 3 returns at most 3 entries

## Failure Criteria
- Non-200 status code
- Response is not a JSON array
- Array length exceeds the requested limit
- Any entry missing `message_id`, `tracker_name`, `tracker_type`, or `detected_at`
- `tracker_type` contains an unexpected value

## Notes
Confirms that the trackers list endpoint correctly scopes results to the account, respects the `limit` parameter, and returns all required per-detection fields.
