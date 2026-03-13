# GC-411: Privacy report with custom days parameter (7 days)

## Metadata
- **Type**: positive
- **Priority**: P1
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
- At least one synced account with messages spanning more than 7 days (source: prior sync)
- Some messages within the last 7 days and some older than 7 days, at least some with tracker domains

## Steps
1. Fetch the privacy report for the last 30 days as a baseline
   - **Target**: `GET /api/privacy/report?account_id=1&days=30`
   - **Input**: account_id = 1, days = 30
   - **Expected**: 200 OK with `total_messages_scanned` = N₃₀ and `tracking_pixels_found` + `link_trackers_found` = T₃₀

2. Fetch the privacy report for the last 7 days
   - **Target**: `GET /api/privacy/report?account_id=1&days=7`
   - **Input**: account_id = 1, days = 7
   - **Expected**: 200 OK with `total_messages_scanned` = N₇ where N₇ ≤ N₃₀

3. Verify the 7-day window scans fewer messages than the 30-day window
   - **Target**: Comparison of step 1 and step 2 responses
   - **Input**: Compare `total_messages_scanned` values
   - **Expected**: N₇ ≤ N₃₀ — the shorter window should not scan more messages than the longer window

4. Verify the comparison object reflects the correct previous period
   - **Target**: Response JSON `comparison` field from step 2
   - **Input**: Inspect comparison sub-object
   - **Expected**: The comparison references the 7-day period before the current 7-day window (i.e., days 8–14 ago), not a 30-day period

## Success Criteria
- [ ] Both requests return 200
- [ ] `total_messages_scanned` for days=7 is ≤ `total_messages_scanned` for days=30
- [ ] Tracker counts for days=7 are ≤ tracker counts for days=30
- [ ] Response shape is identical regardless of `days` value
- [ ] `comparison` object reflects the correct prior period window

## Failure Criteria
- Either request returns non-200
- `total_messages_scanned` for days=7 exceeds that for days=30
- Response shape differs between the two requests
- The `days` parameter is ignored (both responses return identical data)

## Notes
The `days` parameter controls the scan window. A shorter window should yield fewer or equal messages and trackers. This test uses the 30-day baseline to establish an upper bound for the 7-day result.
