# GC-408: Security — SQL Injection in Message ID Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: link-safety
- **Tags**: links, safety, scanning, sql-injection, security, message-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- No specific message required — payloads are injected in the URL path

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt classic SQL injection in message ID
   - **Target**: `POST http://127.0.0.1:3000/api/messages/1' OR '1'='1/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 404 Not Found or 400 Bad Request — server rejects the non-existent ID cleanly; no database error exposed

3. Attempt UNION-based injection in message ID
   - **Target**: `POST http://127.0.0.1:3000/api/messages/1 UNION SELECT 1,2,3--/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 404 Not Found or 400 Bad Request — server treats the entire path segment as an opaque string ID, finds no match

4. Attempt stacked query injection
   - **Target**: `POST http://127.0.0.1:3000/api/messages/msg-id'; DROP TABLE messages;--/scan-links`
   - **Input**: Header `X-Session-Token: {token}`; no request body
   - **Expected**: 404 or 400 — the `messages` table is not dropped; subsequent API calls still work normally

5. Verify the database is intact after injection attempts
   - **Target**: `GET http://127.0.0.1:3000/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a valid messages list — confirms the `messages` table was not affected

6. Verify no internal error details are leaked in any error response
   - **Target**: (inspect response bodies from steps 2, 3, 4)
   - **Input**: response bodies
   - **Expected**: error bodies do not contain SQL keywords, stack traces, table names, or any database internals — only a generic error message

## Success Criteria
- [ ] Steps 2, 3, and 4 each return 404 or 400, never 200 or 500
- [ ] Step 5 returns 200 with a valid message list (database intact)
- [ ] No SQL error text, stack trace, or schema information appears in any error response body
- [ ] Server does not crash or become unresponsive after injection attempts

## Failure Criteria
- Any injection payload returns 200 with scan results (query manipulation succeeded)
- Server returns 500 with SQL error text visible in the response body
- Step 5 fails — messages table was dropped or corrupted by a stacked query
- Server becomes unresponsive after injection attempts
- Response body leaks internal table names, column names, or file paths

## Notes
The message ID is a path parameter passed to a SQL query to look up the message body. The implementation must use parameterized queries (prepared statements) rather than string interpolation. This test verifies that the ID is treated as a literal value and never interpreted as SQL. The URL-encoded forms of these payloads (e.g. `%27` for `'`) should also be considered in a follow-up security audit.
