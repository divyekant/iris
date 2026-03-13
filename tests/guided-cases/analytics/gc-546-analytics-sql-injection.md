# GC-546: SQL Injection in Analytics Query Parameters

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: analytics
- **Tags**: analytics, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious query parameter payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE messages; --`
  - `1 UNION SELECT * FROM config`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Volume with SQL injection in period
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=' OR '1'='1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request (invalid period), NOT 200 with data or 500

3. Volume with injection in days
   - **Target**: `GET http://localhost:3030/api/analytics/volume?period=daily&days=1; DROP TABLE messages`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request (invalid days), NOT 500

4. Overview path traversal attempt
   - **Target**: `GET http://localhost:3030/api/analytics/../messages`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found or valid messages response (path normalized), NOT server error

5. Verify database integrity
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes

## Success Criteria
- [ ] All injection payloads return 400 or 404 (safe rejection)
- [ ] No 500 Internal Server Error
- [ ] No data leaked from other tables
- [ ] Database remains intact

## Failure Criteria
- Any injection returns 200 with unexpected data
- Any injection returns 500
- Database tables dropped or corrupted

## Notes
Analytics query parameters (period, days) are typically validated and parsed as enums/integers before reaching SQL. This verifies the parsing layer rejects injection attempts.
