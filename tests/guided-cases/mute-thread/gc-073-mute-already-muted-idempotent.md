# gc-mute-006: Mute an Already-Muted Thread (Idempotent)

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: idempotent, boundary, INSERT-OR-IGNORE, PUT, thread, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Thread ID: `thread-test-mute-006` (source: inline)
- Thread is NOT currently muted

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Mute the thread (first time)
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-006/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

3. Mute the same thread again (second time)
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-006/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}` (no error, idempotent)

4. Mute the same thread a third time
   - **Target**: `PUT http://localhost:3030/api/threads/thread-test-mute-006/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}` (still idempotent)

5. Verify mute status is still true
   - **Target**: `GET http://localhost:3030/api/threads/thread-test-mute-006/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}`

6. Verify only one entry in muted-threads list for this thread
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array containing `"thread-test-mute-006"` exactly once (no duplicates)

## Success Criteria
- [ ] All three PUT requests return 200 with `{"muted": true}` (no error on repeat)
- [ ] No 409 Conflict or 500 error on duplicate mute
- [ ] Thread appears exactly once in the muted-threads list (INSERT OR IGNORE prevents duplicates)
- [ ] muted_at timestamp is not updated on repeat mutes (original timestamp preserved)

## Failure Criteria
- Any PUT returns non-200 status (e.g., 409 Conflict, 500 Server Error)
- Thread appears more than once in the muted list
- Database constraint violation error
