# GC-278: SQL Injection in Email Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, security, sql-injection, input-validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Malicious email-style payloads (source: inline):
  - `' OR '1'='1`
  - `'; DROP TABLE vip_contacts; --`
  - `" OR ""="`
  - `admin'--`

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. GET vip-score with SQL injection payload in email path (classic OR injection)
   - **Target**: `GET http://localhost:3000/api/contacts/%27%20OR%20%271%27%3D%271/vip-score`
   - **Input**:
     ```
     curl -s -w "\n%{http_code}" \
       "http://localhost:3000/api/contacts/%27%20OR%20%271%27%3D%271/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK with zero-score response (the payload is treated as a literal email string that doesn't exist) OR 400 Bad Request — NOT a 200 with data from other rows, NOT a 500

3. PUT vip with DROP TABLE injection payload
   - **Target**: `PUT http://localhost:3000/api/contacts/%27%3B%20DROP%20TABLE%20vip_contacts%3B%20--/vip`
   - **Input**:
     ```
     curl -s -w "\n%{http_code}" -X PUT \
       "http://localhost:3000/api/contacts/%27%3B%20DROP%20TABLE%20vip_contacts%3B%20--/vip" \
       -H "X-Session-Token: {token}" \
       -H "Content-Type: application/json" \
       -d '{"is_vip": true}'
     ```
   - **Expected**: 200 OK (inserts/updates a row with the injection string as the email literal) OR 400 Bad Request — NOT a 500, and `vip_contacts` table must remain intact

4. Verify vip_contacts table is intact after injection attempts
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, table is accessible and returns normal data (not destroyed)

5. GET vip-score with double-quote injection
   - **Target**: `GET http://localhost:3000/api/contacts/%22%20OR%20%22%22%3D%22/vip-score`
   - **Input**:
     ```
     curl -s -w "\n%{http_code}" \
       "http://localhost:3000/api/contacts/%22%20OR%20%22%22%3D%22/vip-score" \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK with zero-score response (payload treated as literal) — NOT 500, NOT data from other rows

6. Verify server health after all injection attempts
   - **Target**: `GET http://localhost:3000/api/health`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/health
     ```
   - **Expected**: 200 OK, server is healthy, database is accessible

## Success Criteria
- [ ] No injection payload causes a 500 Internal Server Error
- [ ] No injection payload returns data from rows other than the exact email string queried
- [ ] The `vip_contacts` table is not dropped or corrupted
- [ ] The `vip_contacts` endpoint remains functional after all injection attempts
- [ ] Server health check passes after all injection attempts
- [ ] No SQL error messages or stack traces in response bodies

## Failure Criteria
- Any injection payload returns 500 (indicates raw SQL string interpolation)
- Any injection payload returns data from unintended rows (e.g., OR injection succeeds)
- `vip_contacts` table is dropped or becomes inaccessible
- Response body contains a database error string or internal Rust panic output

## Notes
Rust's `rusqlite` uses parameterized queries exclusively (`?1`, `?2` placeholders). The `get_vip_score` handler passes the email through `rusqlite::params![email_lower]`, which prevents injection. The `set_vip` handler similarly uses `rusqlite::params![email_lower]`. Injection payloads should be stored verbatim as email strings (or not found), never interpreted as SQL. This test verifies that the parameterization is actually in use rather than any accidental string formatting.
