# GC-267: SQL Injection in Deadline ID Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, security, sql-injection, path-param
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- SQL injection payloads to use as deadline ID path parameters (source: inline):
  - `1 OR 1=1`
  - `1; DROP TABLE deadlines;--`
  - `' OR 'a'='a`
  - `1 UNION SELECT * FROM messages--`

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt SQL injection via the complete endpoint
   - **Target**: `PUT http://localhost:3030/api/deadlines/{id}/complete`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X PUT \
       "http://localhost:3030/api/deadlines/1%20OR%201%3D1/complete" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — the server rejects or safely fails on a malformed ID; no SQL execution occurs

3. Attempt SQL injection via the delete endpoint
   - **Target**: `DELETE http://localhost:3030/api/deadlines/{id}`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X DELETE \
       "http://localhost:3030/api/deadlines/1%3B%20DROP%20TABLE%20deadlines%3B--" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — not 500, and no DROP TABLE executed

4. Verify the deadlines table still exists after injection attempts
   - **Target**: `GET http://localhost:3030/api/deadlines`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" \
       "http://localhost:3030/api/deadlines" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK — the endpoint still responds, confirming no DDL was executed

5. Attempt UNION-based injection via delete endpoint
   - **Target**: `DELETE http://localhost:3030/api/deadlines/{id}`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X DELETE \
       "http://localhost:3030/api/deadlines/1%20UNION%20SELECT%20*%20FROM%20messages--" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — not 500 or 200

## Success Criteria
- [ ] All injection attempts return 400 or 404, never 500
- [ ] No SQL statements from the payloads are executed (deadlines table intact after step 4)
- [ ] Response bodies do not contain SQL error messages or table schema details
- [ ] Server remains fully functional after all injection attempts

## Failure Criteria
- Any request returns 500 Internal Server Error with a SQL error message in the body
- The `deadlines` table is dropped or altered as a result of the injection payloads
- A UNION-based query leaks data from other tables in the response
- Server crashes or becomes unresponsive after injection attempts

## Notes
Iris uses rusqlite with parameterized queries via `?` placeholders. Path parameters are bound as typed values (typically parsed as integers or UUIDs), not interpolated as raw strings. This structural use of prepared statements should prevent SQL injection at the handler layer. This test confirms that protection holds for the deadline ID path parameter on both `PUT .../complete` and `DELETE` endpoints.
