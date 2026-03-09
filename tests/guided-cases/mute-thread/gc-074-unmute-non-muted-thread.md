# gc-mute-007: Unmute a Non-Muted Thread

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: unmute, boundary, no-op, DELETE, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Thread ID: `thread-test-mute-007-never-muted` (source: inline)
- Thread has NEVER been muted (no row in muted_threads table)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Verify thread is not muted (baseline)
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-007-never-muted/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

3. Unmute the non-muted thread
   - **Target**: `DELETE http://localhost:3030/api/threads/thread-test-mute-007-never-muted/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}` (no error, graceful no-op)

4. Verify status remains unchanged
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-007-never-muted/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

## Success Criteria
- [ ] DELETE on a non-muted thread returns 200 (not 404 or 500)
- [ ] Response body is `{"muted": false}`
- [ ] No database error or crash from DELETE WHERE on non-existent row
- [ ] Mute status remains false after the no-op unmute

## Failure Criteria
- DELETE returns 404 Not Found (treating missing mute as resource-not-found)
- DELETE returns 500 Internal Server Error
- Mute status changes unexpectedly
