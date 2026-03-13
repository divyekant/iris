# GC-526: SQL Injection in Newsletter Feed ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-feeds
- **Tags**: newsletter-feeds, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious feed ID payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE newsletter_feeds; --`
  - `1 UNION SELECT * FROM messages`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET articles with SQL injection in feed ID
   - **Target**: `GET http://localhost:3030/api/newsletter-feeds/' OR '1'='1/articles`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 with data

3. PUT update with DROP TABLE injection
   - **Target**: `PUT http://localhost:3030/api/newsletter-feeds/'; DROP TABLE newsletter_feeds; --`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"name": "test"}`
   - **Expected**: 404 Not Found, NOT 200 or 500

4. DELETE with UNION injection
   - **Target**: `DELETE http://localhost:3030/api/newsletter-feeds/1 UNION SELECT * FROM messages`
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

## Notes
Rust's rusqlite uses parameterized queries by default. This verifies feed ID path parameters are properly parameterized.
