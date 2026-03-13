# GC-595: Delete MCP Session Closes and Removes It

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, session, delete, DELETE, lifecycle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session (session_id known)

## Steps
1. Obtain session token and initialize MCP session
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap` then `POST http://localhost:3030/api/mcp/initialize`
   - **Expected**: Both succeed; `session_id` obtained

2. Delete the session
   - **Target**: `DELETE http://localhost:3030/api/mcp/sessions/{session_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK or 204 No Content

3. Verify session is gone from active sessions list
   - **Target**: `GET http://localhost:3030/api/mcp/sessions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `session_id` no longer present in `sessions` array

4. Verify tool calls on deleted session are rejected
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body with `session_id` of deleted session
   - **Expected**: 404 Not Found or 401/403 (session expired/not found)

## Success Criteria
- [ ] DELETE returns 200 or 204
- [ ] Session no longer listed after deletion
- [ ] Tool calls against deleted session return 404 or session-not-found error
- [ ] Session history is either preserved read-only or also deleted (document actual behavior)

## Failure Criteria
- Session still listed after deletion
- Tool calls still succeed against deleted session
- DELETE returns 5xx
