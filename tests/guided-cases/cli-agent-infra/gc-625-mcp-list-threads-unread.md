# GC-625: MCP list_threads with unread=true Filter Returns Only Unread Threads

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: cli-agent-infra
- **Tags**: mcp, list_threads, filter, unread, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 2 unread threads and at least 1 read thread present in the inbox

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize MCP session
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK, `session_id` returned

3. Call list_threads with unread filter
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "list_threads",
       "arguments": {"unread": true, "limit": 20}
     }
     ```
   - **Expected**: 200 OK, `result` is an array of thread objects

4. Verify all returned threads are unread
   - **Target**: each object in the `result` array
   - **Expected**: Every thread has `is_read: false` (or `unread_count > 0`); no fully-read threads appear in the list

5. Call list_threads without the unread filter and compare count
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: body `{"session_id": "{session_id}", "tool_name": "list_threads", "arguments": {"limit": 20}}`
   - **Expected**: 200 OK, result count is greater than or equal to the filtered count from step 3

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] All threads in filtered result have unread status
- [ ] No read threads included in the `unread=true` result
- [ ] Unfiltered call returns more (or equal) results than filtered call
- [ ] Each result contains at minimum: `thread_id`, `subject`, `from`, `date`, `unread_count`

## Failure Criteria
- Tool call returns error for valid filter arguments
- Read threads appear in the `unread=true` result
- `result` is empty despite known unread threads existing
- Response missing required thread fields
