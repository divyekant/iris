# GC-626: MCP get_thread_summary Returns AI-Generated Summary for Multi-Message Thread

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: cli-agent-infra
- **Tags**: mcp, get_thread_summary, ai, summary, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and reachable

### Data
- A thread with 3 or more messages exists; its `thread_id` is known

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize MCP session
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK, `session_id` returned

3. Call get_thread_summary
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "get_thread_summary",
       "arguments": {"thread_id": "{known_multi_message_thread_id}"}
     }
     ```
   - **Expected**: 200 OK, `result` contains a `summary` string field

4. Verify summary quality
   - **Target**: `result.summary` from step 3
   - **Expected**: Summary is non-empty, at least 30 characters, and not a raw error message

5. Call get_thread_summary again for same thread (cache check)
   - **Target**: same request as step 3
   - **Expected**: 200 OK, returns same or equivalent summary; response time should be faster (cached) or at most the same

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] `result.summary` is non-empty (>30 chars)
- [ ] Summary text is coherent and not an error message
- [ ] Second call for same thread returns successfully (cache hit or fresh generation)
- [ ] Tool call is recorded in session history

## Failure Criteria
- Tool call returns error for valid `thread_id`
- `result.summary` is empty or missing
- Summary is a raw error string or JSON blob
- Second call fails or panics
