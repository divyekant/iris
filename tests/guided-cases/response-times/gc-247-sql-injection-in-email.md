# GC-247: SQL injection attempt in email parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, security, sql-injection, input-sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Attempt SQL injection via email path parameter
   - **Target**: `GET /api/contacts/x@x.com' OR '1'='1/response-times`
   - **Input**: email = `x@x.com' OR '1'='1` (classic SQL injection payload)
   - **Expected**: 200 OK with zero/null stats (treated as literal email, no data found) OR 400 Bad Request

2. Attempt SQL injection with UNION SELECT
   - **Target**: `GET /api/contacts/x@x.com' UNION SELECT 1,2,3--/response-times`
   - **Input**: email = `x@x.com' UNION SELECT 1,2,3--`
   - **Expected**: 200 OK with zero/null stats OR 400 Bad Request — no database error or data leakage

3. Attempt SQL injection with semicolon command
   - **Target**: `GET /api/contacts/x@x.com'; DROP TABLE messages;--/response-times`
   - **Input**: email = `x@x.com'; DROP TABLE messages;--`
   - **Expected**: 200 OK with zero/null stats OR 400 Bad Request — messages table intact

## Success Criteria
- [ ] No 500 errors from any injection attempt
- [ ] No database errors exposed in response bodies
- [ ] All responses are either 200 (zero stats) or 400 (validation rejection)
- [ ] Messages table remains intact after all attempts
- [ ] No unintended data returned (no rows from other tables)

## Failure Criteria
- 500 Internal Server Error with SQL error details
- Data from other tables leaked via UNION injection
- Database table dropped or modified
- Raw SQL error messages exposed to client

## Notes
Rust's rusqlite uses parameterized queries by default, which should prevent SQL injection. This test confirms that the email path parameter is properly bound as a parameter and never interpolated into SQL strings. The LIKE clause used for to/cc matching is a particular area to verify — LIKE metacharacters (%, _) should be escaped.
