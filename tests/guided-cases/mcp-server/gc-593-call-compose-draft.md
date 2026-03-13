# GC-593: Call compose_draft Tool via MCP Creates a Draft

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, tools, call, compose-draft, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session; a configured email account

## Steps
1. Obtain session token and initialize MCP session
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap` then `POST http://localhost:3030/api/mcp/initialize`
   - **Expected**: Both succeed; `session_id` obtained

2. Call the compose_draft tool
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "compose_draft",
       "arguments": {
         "to": ["recipient@example.com"],
         "subject": "MCP Test Draft",
         "body": "This draft was created via MCP tool call."
       }
     }
     ```
   - **Expected**: 200 OK, `result` includes `draft_id` and `status: "saved"`

3. Verify draft exists in the drafts API
   - **Target**: `GET http://localhost:3030/api/drafts/{draft_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, draft matches the composed fields

## Success Criteria
- [ ] Tool call returns 200 OK with `draft_id`
- [ ] Draft is retrievable via the drafts API
- [ ] Draft fields match the compose arguments
- [ ] Tool call recorded in session history

## Failure Criteria
- Tool call fails for valid compose arguments
- Draft not found in drafts API after tool call
- `draft_id` missing from result
