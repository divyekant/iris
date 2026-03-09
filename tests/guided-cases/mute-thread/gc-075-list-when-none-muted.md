# gc-mute-008: List Muted Threads When None Are Muted

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: mute-thread
- **Tags**: list, empty, boundary, GET, muted-threads, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- No threads are currently muted (muted_threads table is empty, or all previously muted threads have been unmuted)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Unmute any potentially muted test threads (cleanup)
   - **Target**: `DELETE http://localhost:3030/api/threads/{id}/mute` for any previously used test thread IDs
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK for each (idempotent unmute)

3. List muted threads
   - **Target**: `GET http://localhost:3030/api/muted-threads`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with body `[]` (empty JSON array)

## Success Criteria
- [ ] GET /api/muted-threads returns 200 OK (not 404 or 204)
- [ ] Response body is an empty JSON array `[]`
- [ ] Content-Type header is `application/json`
- [ ] Response is not null, undefined, or an empty string

## Failure Criteria
- Returns 404 or 204 No Content instead of 200 with empty array
- Returns null or non-array response
- Returns 500 Internal Server Error

## Notes
This verifies that the endpoint gracefully handles the empty-set case by returning an empty array rather than an error or a different status code.
