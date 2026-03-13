# GC-418: SQL injection attempt in account_id parameter is safely rejected

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: privacy-report
- **Tags**: privacy, trackers, report, scanning
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Attempt classic SQL injection via account_id on the report endpoint
   - **Target**: `GET /api/privacy/report?account_id=1 OR 1=1&days=30`
   - **Input**: account_id = `1 OR 1=1`
   - **Expected**: 400 Bad Request (non-integer rejected by parameter parser) OR 200 with empty/zeroed stats treating the value as non-matching — not cross-account data leakage

2. Attempt UNION SELECT injection via account_id
   - **Target**: `GET /api/privacy/report?account_id=1 UNION SELECT 1,2,3,4,5--&days=30`
   - **Input**: account_id = `1 UNION SELECT 1,2,3,4,5--`
   - **Expected**: 400 Bad Request OR 200 with zeroed stats — no data from unintended tables, no SQL error exposed

3. Attempt stacked query injection
   - **Target**: `GET /api/privacy/report?account_id=1; DROP TABLE messages;--&days=30`
   - **Input**: account_id = `1; DROP TABLE messages;--`
   - **Expected**: 400 Bad Request OR 200 with zeroed stats — messages table remains intact after the request

4. Attempt SQL injection on the trackers endpoint
   - **Target**: `GET /api/privacy/trackers?account_id=1' OR '1'='1&limit=20`
   - **Input**: account_id = `1' OR '1'='1`
   - **Expected**: 400 Bad Request OR 200 with empty array — no cross-account data returned

5. Verify the messages table is intact after all injection attempts
   - **Target**: `GET /api/messages?account_id=1`
   - **Input**: Any valid message list request
   - **Expected**: 200 OK with existing messages still present — table was not dropped or modified

## Success Criteria
- [ ] No injection attempt returns a 500 error with SQL error details
- [ ] No injection attempt returns data from unintended accounts or tables
- [ ] No injection attempt modifies or drops any database table
- [ ] Non-integer account_id values are rejected with 400 (parameter type validation)
- [ ] Messages table is intact after all attempts (verified by step 5)
- [ ] Response bodies contain no raw SQL error messages or stack traces

## Failure Criteria
- 500 Internal Server Error with SQL syntax error details in the response body
- Cross-account data returned via UNION injection
- Messages or other tables dropped or modified
- account_id containing SQL keywords parsed as SQL rather than as a string literal
- Raw database error messages (e.g., "near OR: syntax error") exposed to client

## Notes
Rust's rusqlite uses parameterized queries via `?` placeholders by default, which prevents SQL injection when used correctly. This test verifies that account_id is bound as a typed integer parameter (not string-interpolated into the SQL). A non-integer account_id should fail at the parameter parsing layer (serde/axum query extraction) before it ever reaches the SQL layer. The stacked query (step 3) is particularly relevant for SQLite since `execute_batch` could allow multiple statements, but parameterized single-statement queries do not.
