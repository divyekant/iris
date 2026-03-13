# GC-308: SQL injection in reminder ID

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, security, sql-injection, input-sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- None required

## Steps

1. Attempt SQL injection via classic OR payload in reminder ID (snooze endpoint)
   - **Target**: `PUT /api/followups/1' OR '1'='1/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/1%27%20OR%20%271%27%3D%271/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-20"}'
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — the payload is treated as a literal (non-integer) ID, not interpreted as SQL

2. Attempt UNION SELECT injection in reminder ID (dismiss endpoint)
   - **Target**: `PUT /api/followups/1 UNION SELECT 1,2,3--/dismiss`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/1%20UNION%20SELECT%201%2C2%2C3--/dismiss" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — no database error or data leakage in response

3. Attempt DROP TABLE via semicolon injection in reminder ID (acted endpoint)
   - **Target**: `PUT /api/followups/1; DROP TABLE followup_reminders;--/acted`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/1%3B%20DROP%20TABLE%20followup_reminders%3B--/acted" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 400 Bad Request or 404 Not Found — the `followup_reminders` table must remain intact

4. Verify database integrity after injection attempts
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, reminders table still accessible and data is intact

## Success Criteria
- [ ] No 500 errors exposed from any injection attempt
- [ ] No SQL error details appear in any response body
- [ ] All responses are 400 or 404 — never 200 for non-integer IDs
- [ ] `followup_reminders` table remains intact after all attempts
- [ ] No unintended rows updated or deleted across multiple reminders

## Failure Criteria
- 500 Internal Server Error with SQL error details in response body
- Data from other tables leaked via UNION injection
- Multiple reminders affected by a single request (wildcard match via injection)
- `followup_reminders` table dropped or corrupted
- Raw SQL error messages exposed to the client

## Notes
Reminder IDs are expected to be integers. The path parameter should be validated as a numeric type before being used in any query. Rust's rusqlite uses parameterized queries by default, which prevents interpolation-based injection. However, if the ID is parsed as a string and passed into a LIKE clause or dynamically constructed query, injection may still be possible. The DROP TABLE attempt specifically verifies that multi-statement execution is not enabled on the SQLite connection.
