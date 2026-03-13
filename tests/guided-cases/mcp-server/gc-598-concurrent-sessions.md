# GC-598: Concurrent MCP Sessions Are Independent and Isolated

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, session, concurrent, isolation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None required

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize two separate MCP sessions
   - **Target**: `POST http://localhost:3030/api/mcp/initialize` (called twice)
   - **Input**: Header `X-Session-Token: {token}`, same body for both
   - **Expected**: Both return 200 OK with distinct `session_id` values (session_A ≠ session_B)

3. Make a tool call in session_A
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: `session_id: session_A`, `tool_name: "search_emails"`, `arguments: {"query": "test"}`
   - **Expected**: 200 OK; tool call recorded under session_A

4. Verify session_B history is empty (not contaminated by session_A's call)
   - **Target**: `GET http://localhost:3030/api/mcp/sessions/{session_b_id}/history`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `history` array is empty

5. Verify sessions list shows both active sessions
   - **Target**: `GET http://localhost:3030/api/mcp/sessions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Both session_A and session_B appear in `sessions` array

## Success Criteria
- [ ] Two initialize calls produce two distinct session IDs
- [ ] Session histories are isolated (session_B unaffected by session_A's calls)
- [ ] Both sessions appear in the active sessions list
- [ ] No session cross-contamination

## Failure Criteria
- Both initialize calls return the same session_id
- session_B history contains calls made in session_A
- Sessions list shows only one session
