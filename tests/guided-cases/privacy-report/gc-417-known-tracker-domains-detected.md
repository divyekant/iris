# GC-417: Messages with known tracker domains are correctly detected and classified

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
- At least one synced account containing messages with the following in their HTML bodies:
  - A Mailchimp tracking pixel: `<img src="https://mcimg.us2.list-manage.com/track/open.php?...">` (tracker domain: mcimg.com)
  - A HubSpot link tracker: `<a href="https://hs.hubspotlinks.com/...">` (tracker domain: hs.hubspotlinks.com)
  - One clean message with no tracker domains

## Steps
1. Fetch the privacy report to confirm tracker counts are non-zero
   - **Target**: `GET /api/privacy/report?account_id=1&days=30`
   - **Input**: account_id = 1, days = 30
   - **Expected**: 200 OK with `tracking_pixels_found` ≥ 1 and `link_trackers_found` ≥ 1

2. Verify Mailchimp is detected as an OpenPixel tracker in top_trackers
   - **Target**: Response JSON `top_trackers` array
   - **Input**: Search for entry with `name` matching "Mailchimp" or containing "mcimg"
   - **Expected**: Entry present with `type` = "OpenPixel" and `count` ≥ 1

3. Verify HubSpot is detected as a LinkTrack tracker in top_trackers
   - **Target**: Response JSON `top_trackers` array
   - **Input**: Search for entry with `name` matching "HubSpot" or containing "hubspot"
   - **Expected**: Entry present with `type` = "LinkTrack" and `count` ≥ 1

4. Fetch the tracker list and verify individual detections
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=20`
   - **Input**: account_id = 1, limit = 20
   - **Expected**: At least one entry with `tracker_name` matching Mailchimp and `tracker_type` = "OpenPixel"; at least one entry with `tracker_name` matching HubSpot and `tracker_type` = "LinkTrack"

5. Verify clean_messages_percentage reflects the clean message
   - **Target**: Response JSON `clean_messages_percentage`
   - **Input**: Given 1 clean message out of N total messages
   - **Expected**: `clean_messages_percentage` ≈ (1/N) * 100 — not 0 and not 100 when both clean and tracked messages exist

## Success Criteria
- [ ] `tracking_pixels_found` is ≥ 1 when Mailchimp pixel message is in scope
- [ ] `link_trackers_found` is ≥ 1 when HubSpot link message is in scope
- [ ] Mailchimp appears in `top_trackers` with `type` = "OpenPixel"
- [ ] HubSpot appears in `top_trackers` with `type` = "LinkTrack"
- [ ] Individual tracker detections appear in the trackers list with correct types
- [ ] `clean_messages_percentage` is between 0 and 100 (exclusive), reflecting at least one clean message

## Failure Criteria
- `tracking_pixels_found` = 0 despite Mailchimp pixel being present
- `link_trackers_found` = 0 despite HubSpot link being present
- Mailchimp classified as "LinkTrack" instead of "OpenPixel"
- HubSpot classified as "OpenPixel" instead of "LinkTrack"
- `clean_messages_percentage` = 0 when a clean message exists in the period

## Notes
Domain matching must distinguish between pixel trackers (img tags loading from tracker domains) and link trackers (anchor href attributes pointing to tracker domains). mcimg.com is a Mailchimp pixel domain; hs.hubspotlinks.com is a HubSpot redirect/tracking link domain. The classification into OpenPixel vs LinkTrack is based on how the domain appears in the HTML (img src vs anchor href).
