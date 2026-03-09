# GC-085: SQL Injection in Account ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: per-account-notifications
- **Tags**: notifications, security, sql-injection, account-id, input-validation
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious account ID payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE config; --`
  - `" OR ""="`
  - `1; SELECT * FROM config`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET with SQL injection payload in account ID
   - **Target**: `GET http://localhost:3030/api/accounts/' OR '1'='1/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found (account does not exist), NOT 200 with data

3. PUT with SQL injection payload in account ID
   - **Target**: `PUT http://localhost:3030/api/accounts/'; DROP TABLE config; --/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": false}`
   - **Expected**: 404 Not Found, NOT 200 or 500

4. GET with double-quote injection
   - **Target**: `GET http://localhost:3030/api/accounts/" OR ""="/notifications`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

5. PUT with semicolon injection
   - **Target**: `PUT http://localhost:3030/api/accounts/1; SELECT * FROM config/notifications`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"enabled": true}`
   - **Expected**: 404 Not Found

6. Verify config table is intact
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes (DB is accessible and undamaged)

## Success Criteria
- [ ] All injection payloads return 404 (treated as non-existent account IDs)
- [ ] No 200 response with leaked data from injection
- [ ] No 500 Internal Server Error (would indicate unparameterized query)
- [ ] Config table remains intact after all injection attempts (health check passes)
- [ ] No config rows created with injection-derived keys

## Failure Criteria
- Any injection payload returns 200 OK with data
- Any injection payload returns 500 (indicates raw SQL interpolation)
- Config table is dropped or corrupted
- Response body contains SQL error messages or stack traces

## Notes
Rust's rusqlite uses parameterized queries by default (`?1`, `?2` placeholders), which should prevent SQL injection. This case verifies that the account ID path parameter is passed through as a parameter, not interpolated into SQL strings. The account-existence check (`SELECT ... FROM accounts WHERE id = ?1`) should safely handle these payloads and return "not found."
