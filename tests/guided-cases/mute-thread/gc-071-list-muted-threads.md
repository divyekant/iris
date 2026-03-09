# gc-mute-004: List All Muted Threads

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: list, happy-path, GET, muted-threads, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Thread IDs to mute: `thread-test-mute-004a`, `thread-test-mute-004b`, `thread-test-mute-004c` (source: inline)
- All three threads are NOT currently muted

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Mute three threads (setup)
   - **Target**: `PUT http://localhost:3030/api/threads/{id}/mute` for each of `thread-test-mute-004a`, `thread-test-mute-004b`, `thread-test-mute-004c`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": true}` for each request

3. List all muted threads
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array containing all three thread IDs: `["thread-test-mute-004a", "thread-test-mute-004b", "thread-test-mute-004c"]` (order may vary)

4. Unmute one thread and re-check list
   - **Target**: `DELETE http://localhost:3030/api/threads/thread-test-mute-004b/mute`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `{"muted": false}`

5. Verify updated muted list
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array containing `"thread-test-mute-004a"` and `"thread-test-mute-004c"` but NOT `"thread-test-mute-004b"`

## Success Criteria
- [ ] GET /api/muted-threads returns a JSON array (string[])
- [ ] All three muted thread IDs appear in the initial list
- [ ] After unmuting one, the list no longer includes the unmuted thread ID
- [ ] Remaining two thread IDs are still present

## Failure Criteria
- GET /api/muted-threads returns non-200 or non-array response
- Any muted thread ID missing from the initial list
- Unmuted thread ID still present in the updated list
