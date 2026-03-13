# GC-210: Empty thread_id returns 400 Bad Request

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: validation, empty-input, thread-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Send multi-reply request with empty thread_id
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "" }`
   - **Expected**: 400 Bad Request

2. Verify no AI call was made
   - **Target**: Server logs or response timing
   - **Input**: Observe response latency
   - **Expected**: Response is near-instant (< 100ms), indicating validation rejected before AI provider call

## Success Criteria
- [ ] Response status is 400
- [ ] No `options` array in response body (error response, not valid MultiReplyResponse)

## Failure Criteria
- Status other than 400
- Server attempts AI generation with empty thread_id
- 500 Internal Server Error (indicates missing validation)

## Notes
The handler checks `input.thread_id.trim().is_empty()` before any DB or AI calls. This case verifies the guard clause.
