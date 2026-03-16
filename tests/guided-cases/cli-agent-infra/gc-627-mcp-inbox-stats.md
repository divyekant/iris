# GC-627: MCP get_inbox_stats Returns Total, Unread, and Starred Counts

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: cli-agent-infra
- **Tags**: mcp, get_inbox_stats, counts, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 1 unread message and 1 starred message in the inbox (to produce non-zero counts)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Initialize MCP session
   - **Target**: `POST http://localhost:3030/api/mcp/initialize`
   - **Input**: Header `X-Session-Token: {token}`, body `{"client_name": "test-client", "client_version": "1.0"}`
   - **Expected**: 200 OK, `session_id` returned

3. Call get_inbox_stats
   - **Target**: `POST http://localhost:3030/api/mcp/tools/call`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     {
       "session_id": "{session_id}",
       "tool_name": "get_inbox_stats",
       "arguments": {}
     }
     ```
   - **Expected**: 200 OK, `result` contains count fields

4. Verify required count fields
   - **Target**: `result` object from step 3
   - **Expected**: Contains `total` (integer >= 0), `unread` (integer >= 1), `starred` (integer >= 1)

5. Verify counts are consistent with known inbox state
   - **Target**: `result.unread` and `result.starred`
   - **Expected**: `unread >= 1` (at least one unread message seeded in preconditions), `starred >= 1` (at least one starred message seeded), `total >= unread`

## Success Criteria
- [ ] Tool call returns 200 OK
- [ ] `result` contains `total`, `unread`, and `starred` integer fields
- [ ] `unread` reflects at least the seeded unread count
- [ ] `starred` reflects at least the seeded starred count
- [ ] `total >= unread` (logical consistency)

## Failure Criteria
- Tool call returns error
- `result` missing `total`, `unread`, or `starred` fields
- Any count field is negative
- `unread` is 0 when seeded unread messages exist
