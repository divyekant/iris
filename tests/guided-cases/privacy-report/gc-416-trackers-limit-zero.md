# GC-416: Trackers endpoint with limit=0 returns empty array or default

## Metadata
- **Type**: edge
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
- At least one synced account with tracker detections (source: prior sync or seed data)

## Steps
1. Fetch tracker list with limit=0
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=0`
   - **Input**: account_id = 1, limit = 0
   - **Expected**: Either 200 OK with an empty array `[]` (limit=0 means zero results) OR 400 Bad Request rejecting limit=0 as invalid, OR 200 OK with a server-side default applied (e.g., 20 results)

2. Verify no server error occurs
   - **Target**: Response HTTP status and body
   - **Input**: Check status code
   - **Expected**: Response is 200 or 400 — not 500

3. Fetch tracker list with a negative limit
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=-5`
   - **Input**: account_id = 1, limit = -5
   - **Expected**: Either 400 Bad Request OR 200 OK with a safe default limit applied — not 500, not all rows returned unbounded

4. Fetch tracker list without the limit parameter
   - **Target**: `GET /api/privacy/trackers?account_id=1`
   - **Input**: account_id = 1 (no limit param)
   - **Expected**: 200 OK using the server default (e.g., 20 results), response array length ≤ server default

## Success Criteria
- [ ] limit=0 returns 200 with empty array OR 400 — not 500
- [ ] limit=-5 returns 400 OR 200 with a clamped default — not 500
- [ ] No request causes an unbounded full-table scan returned to the client
- [ ] Omitting limit uses a safe server-side default
- [ ] All responses are valid JSON

## Failure Criteria
- 500 Internal Server Error for limit=0 or limit=-5
- limit=-5 returns all rows in the database without a cap
- Response body is not valid JSON for any of these inputs
- limit=0 triggers a SQL LIMIT 0 that causes a driver error

## Notes
A LIMIT 0 in SQL is valid and returns zero rows, so accepting limit=0 as "empty result" is a reasonable implementation. Alternatively, the server may choose to treat it as invalid. The key requirement is no 500 errors and no unbounded result sets. A negative LIMIT in SQLite is treated as "no limit" — the server must guard against this.
