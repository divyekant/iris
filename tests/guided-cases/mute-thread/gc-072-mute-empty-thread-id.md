# gc-mute-005: Mute with Empty Thread ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: validation, empty-input, negative, PUT, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Thread ID: empty string `""` (source: inline)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt to mute with empty thread ID (URL-encoded as missing segment)
   - **Target**: `PUT http://localhost:3030/api/threads//mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found (route does not match with empty path segment)

3. Attempt to check mute status with empty thread ID
   - **Target**: `GET http://localhost:3030/api/threads//mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

4. Attempt to unmute with empty thread ID
   - **Target**: `DELETE http://localhost:3030/api/threads//mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] PUT with empty thread ID returns 404 (not 200 or 500)
- [ ] GET with empty thread ID returns 404
- [ ] DELETE with empty thread ID returns 404
- [ ] No row inserted into muted_threads table with empty thread_id

## Failure Criteria
- Any request returns 200 OK (accepting empty thread ID)
- Any request returns 500 Internal Server Error (unhandled crash)
- A row with empty thread_id is persisted in the database

## Notes
Axum path routing treats `/api/threads//mute` as a non-matching route since the `{id}` segment is empty, so 404 is the expected behavior. If the framework somehow extracts an empty string, the handler should reject it with 400 Bad Request.
