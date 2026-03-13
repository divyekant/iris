# GC-592: Call read_email Tool via MCP Returns Full Message Content

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, tools, call, read-email, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session; a known `message_id` of an existing email

## Steps
1. Obtain session token and initialize MCP session
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap` then `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `Sec-Fetch-Site: same-origin` for bootstrap; `X-Session-Token: {token}` for initialize
   - **Expected**: Both succeed; `session_id` obtained

2. Call the read_email tool
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "read_email",
       "arguments": {"message_id": "{known_message_id}"}
     }
     ```
   - **Expected**: 200 OK, `result` contains full message details including `subject`, `from`, `to`, `body`, `date`, and `attachments`

3. Verify body content is complete
   - **Expected**: `body` field contains actual email text (not truncated); `attachments` is an array (may be empty)

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] `result.body` is non-empty for a message with content
- [ ] `result` includes `subject`, `from`, `to`, `date`
- [ ] Tool call recorded in session history

## Failure Criteria
- `result.body` is empty or missing
- Tool returns error for valid message_id
- Response truncates message body
