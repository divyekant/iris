# GC-414: Negative days parameter returns 400 or falls back to default

## Metadata
- **Type**: negative
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
- At least one synced account (account_id=1) with messages

## Steps
1. Fetch privacy report with a negative days value
   - **Target**: `GET /api/privacy/report?account_id=1&days=-1`
   - **Input**: account_id = 1, days = -1
   - **Expected**: Either 400 Bad Request with a descriptive validation error OR 200 OK using a safe default (e.g., days=30)

2. Fetch privacy report with days=0
   - **Target**: `GET /api/privacy/report?account_id=1&days=0`
   - **Input**: account_id = 1, days = 0
   - **Expected**: Either 400 Bad Request OR 200 OK with `total_messages_scanned` = 0 (zero-width window scans nothing)

3. Fetch privacy report with a non-integer days value
   - **Target**: `GET /api/privacy/report?account_id=1&days=abc`
   - **Input**: account_id = 1, days = "abc"
   - **Expected**: 400 Bad Request with a parameter validation error

4. Fetch privacy report with an excessively large days value
   - **Target**: `GET /api/privacy/report?account_id=1&days=99999`
   - **Input**: account_id = 1, days = 99999
   - **Expected**: Either 400 Bad Request (if a max cap is enforced) OR 200 OK scanning all available messages (no crash or timeout within a reasonable duration)

## Success Criteria
- [ ] days=-1 returns 400 OR returns 200 with a clamped/default value (not a negative-window query)
- [ ] days=0 returns 400 OR returns 200 with `total_messages_scanned` = 0
- [ ] Non-integer days returns 400
- [ ] No 500 errors from any of these inputs
- [ ] If default fallback is used for days=-1, the response matches the default (e.g., 30-day) report

## Failure Criteria
- 500 Internal Server Error for any of these inputs
- Negative days value triggers a SQL date arithmetic error exposed in the response
- days=0 or days=-1 returns data spanning an unintended time range
- Non-integer days silently parsed as 0 without a validation error

## Notes
A negative days value could produce a future-pointing date range (current date minus negative days = future date), which should return no messages or be rejected outright. The key requirement is no 500 errors and no incorrect data.
