# GC-287: SQL Injection in thread_id

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, security, sql-injection, thread-id, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious `thread_id` payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE messages; --`
  - `' UNION SELECT id, subject, body, 1, 2, 3 FROM messages --`

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit autocomplete request with classic OR injection in thread_id
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "'\'' OR '\''1'\''='\''1",
         "partial_text": "Thanks for your email",
         "cursor_position": 21,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK with empty suggestions (thread not found), OR 400/422 Bad Request — NOT data from other rows leaked

3. Submit autocomplete request with DROP TABLE injection in thread_id
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "'\''; DROP TABLE messages; --",
         "partial_text": "Thanks for your email",
         "cursor_position": 21,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK with empty suggestions OR 400/422 — no 500, messages table intact

4. Submit autocomplete request with UNION SELECT injection in thread_id
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "x'\'' UNION SELECT id, subject, body, 1, 2, 3 FROM messages --",
         "partial_text": "Thanks for your email",
         "cursor_position": 21,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK with empty suggestions OR 400/422 — no rows from messages table returned in suggestions

5. Verify messages table is intact after all injection attempts
   - **Target**: `GET http://localhost:3030/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: $TOKEN`
   - **Expected**: 200 OK — messages table accessible and undamaged

## Success Criteria
- [ ] No 500 errors from any injection payload
- [ ] No SQL error messages exposed in response bodies
- [ ] messages table remains intact after all injection attempts
- [ ] No unauthorized data from other tables or rows leaked in response
- [ ] All responses are 200 (zero/empty suggestions) or 400/422 (validation rejection)

## Failure Criteria
- 500 Internal Server Error exposing SQL details
- Data from other messages leaked in `suggestions` or elsewhere in the response
- Messages table dropped or corrupted after DROP TABLE payload
- Raw SQL error text visible in any response body

## Notes
Rust's rusqlite uses parameterized queries by default (`?1`, `?2` placeholders). The `thread_id` JSON body field should be bound as a query parameter when fetching thread context, never interpolated into SQL strings. This test verifies that guarantee holds for the autocomplete endpoint.
