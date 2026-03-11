# GC-198: Needs-Reply SQL Injection in Account ID Handled Safely

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, security, sql-injection
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Send a SQL injection payload as account_id
   - **Target**: `GET /api/messages/needs-reply?account_id=1%27%20OR%201%3D1%20--`
   - **Input**: Valid `X-Session-Token` header; malicious `account_id` = `1' OR 1=1 --`
   - **Expected**: 200 OK with an empty `messages` array (no account matches the literal string), or 400 Bad Request

2. Send a UNION-based injection payload
   - **Target**: `GET /api/messages/needs-reply?account_id=1%27%20UNION%20SELECT%20*%20FROM%20accounts%20--`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with empty results or 400 Bad Request; no leaked data from other tables

3. Verify server stability
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK; server did not crash

## Success Criteria
- [ ] SQL injection payloads do not return data from other tables
- [ ] No 500 Internal Server Error
- [ ] Server remains healthy after the requests
- [ ] Parameterized queries prevent injection

## Failure Criteria
- Data from unrelated tables appears in the response
- Server returns 500 or crashes
- Error message exposes SQL syntax or database structure
