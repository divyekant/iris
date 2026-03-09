# gc-mute-001: Mute a Thread End-to-End

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: mute, happy-path, PUT, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one synced email thread exists in the database
- Thread ID to mute: `thread-test-mute-001` (source: inline)
- Thread is NOT currently muted

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Mute the thread
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-001/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

3. Verify mute status via GET
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-001/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

4. Verify thread appears in muted list
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array containing `"thread-test-mute-001"`

## Success Criteria
- [ ] PUT /api/threads/{id}/mute returns 200 with `{"muted": true}`
- [ ] GET /api/threads/{id}/mute confirms muted status is true
- [ ] GET /api/muted-threads includes the thread ID in the returned array

## Failure Criteria
- PUT returns non-200 status code
- GET mute status returns `{"muted": false}` after muting
- Thread ID not present in muted-threads list
