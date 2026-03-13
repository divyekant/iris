# GC-237: Contact Topics SQL Injection in Email Parameter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, security, sql-injection
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No specific data required

## Steps
1. Attempt SQL injection via UNION SELECT
   - **Target**: `GET /api/contacts/x@x'%20UNION%20SELECT%20sql%20FROM%20sqlite_master--/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `x@x' UNION SELECT sql FROM sqlite_master--`)
   - **Expected**: 200 OK with empty topics (treated as literal email, no messages found) or 400

2. Attempt SQL injection via OR 1=1
   - **Target**: `GET /api/contacts/x@x'%20OR%20'1'='1/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `x@x' OR '1'='1`)
   - **Expected**: 200 OK with empty topics or 400, NOT data from other contacts

3. Attempt SQL injection via semicolon (stacked queries)
   - **Target**: `GET /api/contacts/x@x';DROP%20TABLE%20messages;--/topics`
   - **Input**: Header `X-Session-Token: {token}` (email: `x@x';DROP TABLE messages;--`)
   - **Expected**: 200 OK with empty topics or 400, messages table unaffected

4. Verify database integrity after injection attempts
   - **Target**: `GET /api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, messages still accessible (table not dropped)

## Success Criteria
- [ ] No SQL injection succeeds — no unauthorized data returned
- [ ] UNION SELECT does not return schema information
- [ ] OR 1=1 does not return all contacts' data
- [ ] DROP TABLE does not execute
- [ ] Messages table remains intact after all attempts
- [ ] All responses are 200 (empty) or 400, never 500

## Failure Criteria
- Response contains sqlite_master schema data
- Response contains topics from contacts other than the injected email
- Messages table is dropped or corrupted
- Server returns 500 (SQL error leaked)

## Notes
Rust's rusqlite uses parameterized queries by default, which should prevent SQL injection. The email parameter is passed as a bound parameter, not interpolated into the SQL string. This case verifies that protection is in place. SQLite also does not support stacked queries via the standard API, providing an additional safety layer.
