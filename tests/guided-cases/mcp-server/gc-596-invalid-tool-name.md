# GC-596: Invalid Tool Name Returns Descriptive Error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, tools, call, invalid-tool, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session

## Steps
1. Obtain session token and initialize MCP session
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap` then `POST http://localhost:3030/api/mcp/initialize`
   - **Expected**: Both succeed; `session_id` obtained

2. Call a non-existent tool
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "delete_all_emails",
       "arguments": {}
     }
     ```
   - **Expected**: 400 Bad Request or 404 Not Found, error message identifies the unknown tool name; no emails deleted

3. Verify error response structure
   - **Expected**: Response includes `error` field with `code` and `message` indicating tool not found; does not expose internal stack traces

## Success Criteria
- [ ] Invalid tool name returns 400 or 404 (not 200 or 5xx)
- [ ] Error message identifies the unrecognized tool
- [ ] No side effects (no emails deleted or modified)
- [ ] Error response does not leak internal implementation details

## Failure Criteria
- Server returns 500 for unknown tool name
- Error message exposes stack trace
- Server attempts to execute unknown tool and causes data corruption
