# GC-193: Boundary — No Notes Empty State

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: boundary, empty-state, no-notes
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced email thread that has never had notes (source: inbox sync)
- Note the `thread_id` of that thread

## Steps
1. List notes for a thread that has never had notes
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id` with no notes
   - **Expected**: 200 with `{ "notes": [] }`

2. Create a note, then delete it
   - **Target**: `POST /api/threads/{thread_id}/notes` then `DELETE /api/threads/{thread_id}/notes/{id}`
   - **Input**: `{ "content": "Temporary note." }` → then delete
   - **Expected**: 201, then 204

3. List notes after the only note was deleted
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200 with `{ "notes": [] }` — returns to empty state

4. Verify response structure is consistent
   - **Target**: Compare responses from steps 1 and 3
   - **Input**: both responses
   - **Expected**: Identical JSON structure (`{ "notes": [] }`)

## Success Criteria
- [ ] Fresh thread with no notes returns `{ "notes": [] }`
- [ ] Thread returns to `{ "notes": [] }` after last note deleted
- [ ] Response structure is consistent between "never had notes" and "all notes deleted"
- [ ] Status code is 200 (not 204 or 404) for empty notes list

## Failure Criteria
- Empty notes list returns null instead of empty array
- Status code is anything other than 200 for empty list
- Response structure differs between "never had notes" and "all deleted"

## Notes
This tests the distinction between "no notes exist" and "notes were deleted" — the API should not distinguish between these states. Both should return an empty array.
