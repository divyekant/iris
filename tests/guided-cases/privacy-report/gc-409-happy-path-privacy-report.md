# GC-409: Happy path — privacy report returns all expected fields

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
- At least one synced account with messages (source: prior sync)
- Some messages in the account contain known tracker domains (e.g., mcimg.com, hs.hubspotlinks.com)

## Steps
1. Fetch the privacy report with default parameters
   - **Target**: `GET /api/privacy/report?account_id=1&days=30`
   - **Input**: account_id = 1, days = 30
   - **Expected**: 200 OK with JSON body containing all required top-level fields

2. Verify all required response fields are present
   - **Target**: Response JSON inspection
   - **Input**: Check field presence and types
   - **Expected**: Response contains `total_messages_scanned` (integer ≥ 0), `tracking_pixels_found` (integer ≥ 0), `link_trackers_found` (integer ≥ 0), `clean_messages_percentage` (float 0–100), `trackers_blocked_percentage` (float 0–100), `top_trackers` (array), `comparison` (object)

3. Verify top_trackers array structure
   - **Target**: Response JSON `top_trackers` field
   - **Input**: Inspect each entry in the array
   - **Expected**: Each entry has `name` (string), `count` (integer > 0), `type` (string: "OpenPixel" or "LinkTrack")

4. Verify comparison object structure
   - **Target**: Response JSON `comparison` field
   - **Input**: Inspect comparison sub-object
   - **Expected**: Contains at minimum a reference to the previous period's tracker counts (e.g., `previous_period_trackers` or delta fields)

## Success Criteria
- [ ] Response status is 200
- [ ] `total_messages_scanned` is a non-negative integer
- [ ] `tracking_pixels_found` is a non-negative integer
- [ ] `link_trackers_found` is a non-negative integer
- [ ] `clean_messages_percentage` is a float in range [0, 100]
- [ ] `trackers_blocked_percentage` is a float in range [0, 100]
- [ ] `top_trackers` is an array (may be empty if no trackers found)
- [ ] Each `top_trackers` entry has `name`, `count`, and `type` fields
- [ ] `type` values are only "OpenPixel" or "LinkTrack"
- [ ] `comparison` object is present

## Failure Criteria
- Non-200 status code
- Any required top-level field missing from response
- `clean_messages_percentage` or `trackers_blocked_percentage` outside [0, 100]
- `top_trackers` entries missing `name`, `count`, or `type`
- `type` value other than "OpenPixel" or "LinkTrack"

## Notes
Primary happy path. Confirms the full privacy report pipeline works end-to-end for an account with message history including known tracker domains.
