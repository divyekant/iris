# GC-257: SQL Injection in message_id Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`

### Data
- SQL injection payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE messages; --`
  - `" OR ""="`
  - `1 UNION SELECT id, subject, body FROM messages --`
  - `\x00` (null byte)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. POST detect-intent with a classic OR injection payload
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -o - -w "\nHTTP_STATUS:%{http_code}" -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "'"'"' OR '"'"'1'"'"'='"'"'1"}'
     ```
   - **Expected**: 404 Not Found (no message matches that literal string as an ID), NOT 200 with message data

3. POST detect-intent with a DROP TABLE injection payload
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -o - -w "\nHTTP_STATUS:%{http_code}" -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "'"'"'; DROP TABLE messages; --"}'
     ```
   - **Expected**: 404 Not Found; messages table must remain intact

4. POST detect-intent with a UNION SELECT injection payload
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -o - -w "\nHTTP_STATUS:%{http_code}" -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "1 UNION SELECT id, subject, body FROM messages --"}'
     ```
   - **Expected**: 404 Not Found; no message data returned from UNION

5. GET stored-intent endpoint with injection payload in URL path
   - **Target**: `GET http://localhost:3000/api/messages/%27%20OR%20%271%27%3D%271/intent`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" \
       "http://localhost:3000/api/messages/%27%20OR%20%271%27%3D%271/intent" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 404 Not Found; no data leaked

6. Verify database integrity after all injection attempts
   - **Target**: `GET http://localhost:3000/api/messages?limit=1`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; messages table is intact and queryable; no rows were deleted or modified

7. Verify server health
   - **Target**: `GET http://localhost:3000/api/health`
   - **Input**: n/a
   - **Expected**: 200 OK; all health checks pass including DB connectivity

## Success Criteria
- [ ] All injection payloads return 404 (treated as non-existent message IDs, not as SQL)
- [ ] No payload returns 200 with message data (no authentication or data bypass)
- [ ] No payload returns 500 (which would indicate unparameterized raw SQL interpolation)
- [ ] Messages table remains intact after the DROP TABLE attempt (parameterized queries prevent execution)
- [ ] No SQL error messages or stack traces appear in any response body
- [ ] Server health check passes after all injection attempts

## Failure Criteria
- Any injection payload returns 200 with message data
- Any injection payload returns 500 (indicates raw SQL interpolation)
- Messages table is dropped or corrupted (DROP TABLE executed)
- Response body contains SQL error text, table names, or schema details
- UNION-based injection returns rows from the messages table

## Notes
Rust's rusqlite uses parameterized queries (`?1` placeholders) by default. The `message_id` JSON body field and URL path parameter should both be passed as bound parameters, not interpolated into SQL strings. This case verifies that invariant holds for both the POST and GET intent endpoints.
