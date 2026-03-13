# GC-597: Tool Call Against Expired Session Returns Session-Not-Found Error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, session, expired, negative, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A known `session_id` that either no longer exists (deleted) or has passed its TTL

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Attempt a tool call using a non-existent session ID
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "00000000-0000-0000-0000-000000000000",
       "tool_name": "search_emails",
       "arguments": {"query": "test"}
     }
     ```
   - **Expected**: 404 Not Found or 401 Unauthorized with error message indicating session not found or expired

3. Retrieve history for the non-existent session
   - **Target**: `GET http://localhost:3030/api/mcp/sessions/00000000-0000-0000-0000-000000000000/history`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Tool call with invalid session_id returns 404 (not 200)
- [ ] Error message clearly indicates session not found
- [ ] History endpoint returns 404 for invalid session
- [ ] No tool execution occurs for invalid session

## Failure Criteria
- Tool call succeeds with fake session_id
- Server returns 200 with empty results for invalid session
- 5xx error instead of 404
