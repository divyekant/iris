# GC-224: Nonexistent Message ID — 404 Not Found

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: not-found, 404, invalid-id, error
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A message_id that does not exist in the database: `nonexistent-msg-id-99999` (source: inline)
- A thread_id that does not exist in the database: `nonexistent-thread-id-99999` (source: inline)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract tasks with a nonexistent message_id
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"message_id": "nonexistent-msg-id-99999"}`
   - **Expected**: 404 Not Found

3. Extract tasks with a nonexistent thread_id
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "nonexistent-thread-id-99999"}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Step 2 returns 404 Not Found
- [ ] Step 3 returns 404 Not Found
- [ ] No AI provider call is made for nonexistent messages
- [ ] Response body does not leak internal database details

## Failure Criteria
- Either request returns 200 with empty tasks (should be 404, not 200 with [])
- Either request returns 500 Internal Server Error
- AI provider is invoked despite no messages found

## Notes
The handler checks `MessageDetail::get_by_id` (returns `None` mapped to 404) for message_id and `MessageDetail::list_by_thread` (returns empty vec, checked with `.is_empty()`) for thread_id. Both paths should return 404 before reaching the AI provider.
