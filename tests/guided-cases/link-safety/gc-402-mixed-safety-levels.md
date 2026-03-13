# GC-402: Mixed Safety Levels — Safe, Caution, and Danger Links in One Message

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, mixed, overall-risk, aggregation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose HTML body contains the following links:
  - Safe: `<a href="https://www.apple.com/support">Apple Support</a>`
  - Caution: `<a href="https://bit.ly/2Xfp9Qu">More info</a>` (URL shortener)
  - Danger: `<a href="http://203.0.113.42/reset-password">Reset Password</a>` (IP address)
- The message ID is known as `{msg_id}`

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Scan links in the message
   - **Target**: `POST http://127.0.0.1:3000/api/messages/{msg_id}/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body required
   - **Expected**: 200 OK with a JSON body containing a `links` array of 3 entries and a `summary` object

3. Verify individual link classifications
   - **Target**: (inspect `links` array from step 2)
   - **Input**: each link entry
   - **Expected**:
     - `apple.com` entry: `safety` = `"safe"`, `is_known_trusted` = `true`
     - `bit.ly` entry: `safety` = `"caution"` or `"danger"`, `is_shortened` = `true`
     - `203.0.113.42` entry: `safety` = `"danger"`, `reasons` references IP address

4. Verify the summary aggregates correctly and escalates to the worst severity
   - **Target**: (inspect `summary` object from step 2)
   - **Input**: `summary` field
   - **Expected**: `total_links` = 3, `safe_count` = 1, at least 1 in `caution_count`, `danger_count` >= 1, `overall_risk` = `"danger"` (danger escalates over caution and safe)

5. Verify each link entry contains the `url` and `domain` fields
   - **Target**: (inspect each entry in `links` array)
   - **Input**: link objects
   - **Expected**: every entry has non-empty `url` matching the original href and a `domain` extracted from the URL

## Success Criteria
- [ ] Response status is 200
- [ ] `links` array has exactly 3 entries
- [ ] `apple.com` link is `"safe"` and `is_known_trusted` = `true`
- [ ] `bit.ly` link has `is_shortened` = `true` and safety at least `"caution"`
- [ ] IP-address link is `"danger"`
- [ ] `summary.total_links` = 3, `summary.safe_count` = 1
- [ ] `summary.overall_risk` = `"danger"` (worst-case escalation)
- [ ] All link objects include `url` and `domain` fields

## Failure Criteria
- Response status is not 200
- `summary.overall_risk` is not escalated to `"danger"` when a danger-level link is present
- Any known-safe domain (apple.com) is flagged as caution or danger
- IP-address link is not flagged as danger
- `links` array is missing entries or misses any href
