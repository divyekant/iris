# GC-536: SQL Injection in Subscription ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-management
- **Tags**: subscriptions, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious subscription ID payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE subscriptions; --`
  - `" OR ""="`
  - `1 UNION SELECT * FROM messages`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. GET subscription with SQL injection in ID
   - **Target**: `GET http://localhost:3030/api/subscriptions/' OR '1'='1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 with data

3. PUT status with DROP TABLE injection
   - **Target**: `PUT http://localhost:3030/api/subscriptions/'; DROP TABLE subscriptions; --/status`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"status": "inactive"}`
   - **Expected**: 404 Not Found, NOT 200 or 500

4. SQL injection in bulk-action IDs
   - **Target**: `POST http://localhost:3030/api/subscriptions/bulk-action`
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"ids": ["' OR '1'='1", "'; DROP TABLE subscriptions; --"], "action": "unsubscribe"}`
   - **Expected**: 404 (IDs not found) or 200 with `{"updated": 0}`, NOT data from other tables

5. Verify database integrity
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes

## Success Criteria
- [ ] All injection payloads handled safely (404 or zero results)
- [ ] No 500 Internal Server Error
- [ ] No data leaked from other tables
- [ ] Database remains intact

## Failure Criteria
- Any injection returns 200 with unexpected data
- Any injection returns 500
- Database tables dropped or corrupted

## Notes
Rust's rusqlite uses parameterized queries by default. This verifies path params and array elements in body are properly parameterized.
