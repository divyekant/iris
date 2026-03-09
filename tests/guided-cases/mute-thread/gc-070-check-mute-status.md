# gc-mute-003: Check Mute Status of Muted and Non-Muted Threads

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: mute-status, happy-path, GET, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Muted thread ID: `thread-test-mute-003a` (source: inline) -- muted via PUT beforehand
- Non-muted thread ID: `thread-test-mute-003b` (source: inline) -- never muted

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Mute thread A (setup)
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-003a/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

3. Check mute status of muted thread
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-003a/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

4. Check mute status of non-muted thread
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-003b/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

## Success Criteria
- [ ] GET /api/threads/{id}/mute returns `{"muted": true}` for a muted thread
- [ ] GET /api/threads/{id}/mute returns `{"muted": false}` for a non-muted thread
- [ ] Both requests return 200 OK (no error for checking non-muted thread)

## Failure Criteria
- Either GET request returns non-200 status code
- Muted thread returns `{"muted": false}`
- Non-muted thread returns `{"muted": true}`
