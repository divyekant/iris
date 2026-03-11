# GC-188: SQL Injection in Message ID Parameter Handled Safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, security, sql-injection
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. POST to the unsubscribe endpoint with a SQL injection payload as the message ID
   - **Target**: POST /api/messages/1%27%20OR%20%271%27%3D%271/unsubscribe
   - **Input**: none (the injection is in the path segment: `1' OR '1'='1`)
   - **Expected**: 404 Not Found (no message matches; injection not executed)

2. Repeat with a different injection variant
   - **Target**: POST /api/messages/1;DROP TABLE messages;--/unsubscribe
   - **Input**: none
   - **Expected**: 404 Not Found or 400 Bad Request; messages table must remain intact

3. Verify the messages table is unaffected
   - **Target**: GET /api/messages?limit=1
   - **Expected**: 200 OK with at least one message (table not dropped)

## Success Criteria
- [ ] Both injection attempts return 404 or 400 (not 200 or 500)
- [ ] Server does not crash or return a DB error trace
- [ ] Messages table is intact after the requests (GET /api/messages returns data)
- [ ] No unsubscribe action is triggered

## Failure Criteria
- Any injection request returns 200
- Server returns 500 with a SQLite error message (indicates un-parameterized query)
- Messages table is empty or inaccessible after the requests
