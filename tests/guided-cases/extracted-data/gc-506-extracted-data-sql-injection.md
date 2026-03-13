# GC-506: SQL Injection in Extracted Data Endpoints

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE extracted_data; --`
  - `1 UNION SELECT * FROM messages`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. POST extract with SQL injection in message ID
   - **Target**: `POST http://localhost:3030/api/extract/' OR '1'='1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found, NOT 200 with data

3. GET extracted data with injection in type filter
   - **Target**: `GET http://localhost:3030/api/extracted-data?type=' UNION SELECT * FROM messages --`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 with empty array or 400 (type not matched), NOT data from other tables

4. DELETE with injection in ID
   - **Target**: `DELETE http://localhost:3030/api/extracted-data/'; DROP TABLE extracted_data; --`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

5. Verify database integrity
   - **Target**: `GET http://localhost:3030/api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, health check passes

## Success Criteria
- [ ] All injection payloads handled safely (404 or empty results)
- [ ] No 500 Internal Server Error
- [ ] No data from other tables leaked via UNION injection
- [ ] Database remains intact

## Failure Criteria
- Any injection returns 200 with unexpected data
- Any injection returns 500
- Database tables dropped or corrupted

## Notes
Rust's rusqlite uses parameterized queries by default. This verifies all path params and query params are properly parameterized.
