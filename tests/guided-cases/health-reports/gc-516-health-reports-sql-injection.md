# GC-516: SQL Injection in Health Report ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: health-reports
- **Tags**: health-reports, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious report ID payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE health_reports; --`
  - `1 UNION SELECT * FROM messages`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET with SQL injection in report ID
   - **Target**: `GET http://localhost:3030/api/health-reports/' OR '1'='1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 with data

3. DELETE with DROP TABLE injection
   - **Target**: `DELETE http://localhost:3030/api/health-reports/'; DROP TABLE health_reports; --`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 or 500

4. GET with UNION injection
   - **Target**: `GET http://localhost:3030/api/health-reports/1 UNION SELECT * FROM messages`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

5. Verify database integrity
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes

## Success Criteria
- [ ] All injection payloads return 404
- [ ] No 200 with leaked data
- [ ] No 500 Internal Server Error
- [ ] Database remains intact

## Failure Criteria
- Any injection returns 200 with data
- Any injection returns 500
- Database tables dropped or corrupted
- Response body contains SQL error messages

## Notes
Rust's rusqlite uses parameterized queries by default. This verifies report ID path parameters are properly parameterized.
