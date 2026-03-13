# GC-212: Nonexistent thread_id returns 404 Not Found

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: validation, not-found, thread-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and enabled

### Data
- A thread_id that does not exist in the database (e.g., `nonexistent-thread-xyz-999`)

## Steps
1. Send multi-reply request with a nonexistent thread_id
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "nonexistent-thread-xyz-999" }`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Response status is 404
- [ ] No AI provider call is made (validation fails before generation)

## Failure Criteria
- Status other than 404
- Server returns 500 (indicates unhandled error in thread lookup)
- Server attempts AI generation with empty message list

## Notes
After passing the empty-string check, the handler calls `MessageDetail::list_by_thread()` and checks `messages.is_empty()`. A nonexistent thread_id returns an empty vec, triggering the 404.
