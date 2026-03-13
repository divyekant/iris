# GC-496: SQL Injection in Webhook ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: webhooks
- **Tags**: webhooks, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious webhook ID payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE webhooks; --`
  - `" OR ""="`
  - `1; SELECT * FROM webhooks`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET with SQL injection in webhook ID
   - **Target**: `GET http://localhost:3030/api/webhooks/' OR '1'='1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 with data

3. DELETE with DROP TABLE injection
   - **Target**: `DELETE http://localhost:3030/api/webhooks/'; DROP TABLE webhooks; --`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 or 500

4. POST test with injection payload
   - **Target**: `POST http://localhost:3030/api/webhooks/" OR ""="/test`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

5. SQL injection in URL body field
   - **Target**: `POST http://localhost:3030/api/webhooks`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"url": "'; DROP TABLE webhooks; --", "events": ["message.received"]}`
   - **Expected**: 400 Bad Request (invalid URL format) or 201 with safely stored string

6. Verify database integrity
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes

## Success Criteria
- [ ] All injection payloads in path return 404
- [ ] No 200 with leaked data from injection
- [ ] No 500 Internal Server Error
- [ ] Database remains intact after all attempts
- [ ] URL injection in body either rejected or safely stored (not executed as SQL)

## Failure Criteria
- Any path injection returns 200 with data
- Any injection returns 500 (raw SQL interpolation)
- Database tables dropped or corrupted
- Response body contains SQL error messages

## Notes
Rust's rusqlite uses parameterized queries by default, which should prevent SQL injection. This case verifies path parameters and body fields are properly parameterized.
