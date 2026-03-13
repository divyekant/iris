# GC-192: Validation — Empty Thread ID Rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: validation, thread-id, empty
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No specific data required beyond server availability

## Steps
1. Attempt to list notes with empty thread_id
   - **Target**: `GET /api/threads//notes` (empty path segment)
   - **Input**: empty `thread_id`
   - **Expected**: 400 Bad Request or 404 Not Found (route mismatch)

2. Attempt to create a note with empty thread_id
   - **Target**: `POST /api/threads//notes`
   - **Input**: `{ "content": "This should fail." }`
   - **Expected**: 400 Bad Request or 404 Not Found

3. Attempt to list notes with a nonexistent thread_id
   - **Target**: `GET /api/threads/nonexistent-thread-id-12345/notes`
   - **Input**: fabricated `thread_id`
   - **Expected**: 200 with `{ "notes": [] }` (empty list, not an error — thread may simply have no notes)

## Success Criteria
- [ ] Empty thread_id on list returns 400 or 404
- [ ] Empty thread_id on create returns 400 or 404
- [ ] Nonexistent thread_id on list returns 200 with empty notes array

## Failure Criteria
- Server returns 500 for empty thread_id
- A note is created without a valid thread_id
- Nonexistent thread_id returns an error instead of an empty list

## Notes
The distinction between "empty" and "nonexistent" thread IDs matters: empty is a client error (malformed request), while a nonexistent thread simply has no notes yet. The API should handle both gracefully.
