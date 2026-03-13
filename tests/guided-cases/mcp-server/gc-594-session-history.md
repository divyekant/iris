# GC-594: Session History Tracks All Tool Calls in Order

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, session, history, GET, audit
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session with at least 2 tool calls already made (e.g., search_emails followed by read_email)

## Steps
1. Obtain session token and initialize MCP session
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap` then `POST http://localhost:3030/api/mcp/initialize`
   - **Expected**: Both succeed; `session_id` obtained

2. Make two tool calls
   - Call `search_emails` then `read_email` via `POST http://localhost:3030/api/mcp/tools/call`
   - **Expected**: Both return 200 OK

3. Retrieve session history
   - **Target**: `GET http://localhost:3030/api/mcp/sessions/{session_id}/history`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `history` array has exactly 2 entries in chronological order; each entry includes `tool_name`, `arguments`, `result_summary`, and `called_at` timestamp

## Success Criteria
- [ ] History returns 200 OK
- [ ] History contains all tool calls made in this session
- [ ] Order is chronological (oldest first)
- [ ] Each history entry has `tool_name`, `called_at`, and result info
- [ ] `arguments` in history matches what was submitted

## Failure Criteria
- History missing tool calls that were made
- History out of order
- `called_at` timestamps missing or incorrect
