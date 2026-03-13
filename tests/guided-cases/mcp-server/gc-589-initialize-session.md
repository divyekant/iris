# GC-589: Initialize MCP Session Returns Session ID

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, session, initialize, POST, happy-path
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

2. Initialize an MCP session
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK or 201 Created, response includes `session_id` (UUID or similar), `created_at`, and `available_tools` list

3. Verify session appears in active sessions list
   - **Target**: `GET http://localhost:3030/api/mcp/sessions`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `sessions` array contains an entry with the returned `session_id`

## Success Criteria
- [ ] Initialize returns 200 or 201 with `session_id`
- [ ] `session_id` is unique and non-empty
- [ ] New session appears in sessions list
- [ ] Response includes `available_tools` or `tool_count`

## Failure Criteria
- Initialize returns error for valid request
- `session_id` missing from response
- Session not listed after initialization
