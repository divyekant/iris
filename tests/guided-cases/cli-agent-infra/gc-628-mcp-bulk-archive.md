# GC-628: MCP bulk_action Archives Multiple Messages by ID

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: cli-agent-infra
- **Tags**: mcp, bulk_action, archive, multi-message, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 2 messages in the Inbox folder; their `message_id` values are known (obtain via list_threads or search_emails)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize MCP session
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK, `session_id` returned

3. Call bulk_action to archive two messages
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "bulk_action",
       "arguments": {
         "action": "archive",
         "message_ids": ["{message_id_1}", "{message_id_2}"]
       }
     }
     ```
   - **Expected**: 200 OK, `result` indicates success with a count or per-message status

4. Verify result reports both messages processed
   - **Target**: `result` from step 3
   - **Expected**: `result.archived_count == 2` (or equivalent), no error entries for the valid IDs

5. Verify messages no longer appear in Inbox
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call` with `tool_name: "list_threads"` and folder filter `"inbox"`
   - **Expected**: Archived message IDs are absent from the inbox listing; they may appear under an Archive folder if queried

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] Result confirms both messages were archived (count = 2 or per-ID success)
- [ ] Archived messages no longer appear in Inbox
- [ ] Tool call logged in session history
- [ ] No partial failure for valid message IDs

## Failure Criteria
- Tool call returns error for valid message IDs
- Result reports fewer than 2 archived despite 2 valid IDs provided
- Messages remain in Inbox after successful response
- Non-existent ID causes entire batch to fail (should process valid IDs)
