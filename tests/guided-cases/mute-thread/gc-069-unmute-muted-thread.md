# gc-mute-002: Unmute a Previously Muted Thread

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: unmute, happy-path, DELETE, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Thread ID: `thread-test-mute-002` (source: inline)
- Thread is currently muted (mute it via `PUT /api/threads/thread-test-mute-002/mute` beforehand)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Mute the thread (setup)
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-002/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

3. Unmute the thread
   - **Target**: `DELETE http://localhost:3030/api/threads/thread-test-mute-002/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

4. Verify mute status via GET
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-002/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

5. Verify thread is removed from muted list
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array that does NOT contain `"thread-test-mute-002"`

## Success Criteria
- [ ] DELETE /api/threads/{id}/mute returns 200 with `{"muted": false}`
- [ ] GET /api/threads/{id}/mute confirms muted status is false after unmuting
- [ ] GET /api/muted-threads no longer includes the thread ID

## Failure Criteria
- DELETE returns non-200 status code
- GET mute status returns `{"muted": true}` after unmuting
- Thread ID still present in muted-threads list after unmute
