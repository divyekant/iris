# GC-591: Call search_emails Tool via MCP Returns Matching Results

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, tools, call, search-emails, POST
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An active MCP session (session_id from GC-589 or a fresh initialize call)
- At least one email about "budget planning" in the inbox

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize MCP session (or reuse existing)
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK, `session_id` returned

3. Call the search_emails tool
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "search_emails",
       "arguments": {"query": "budget planning", "limit": 5}
     }
     ```
   - **Expected**: 200 OK, `result` contains an array of matching email summaries with `message_id`, `subject`, `from`, `date` fields

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] `result` array is non-empty for a known query
- [ ] Results include required email fields
- [ ] Tool call is logged in session history

## Failure Criteria
- Tool call returns error for valid arguments
- `result` empty despite matching emails in inbox
- Response missing `result` field
