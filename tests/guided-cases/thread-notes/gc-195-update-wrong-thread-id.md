# GC-195: Negative — Update Note with Mismatched Thread ID

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: negative, update, thread-id-mismatch, authorization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Two synced email threads exist (source: inbox sync)
- Note `thread_id_A` and `thread_id_B` for the two threads

## Steps
1. Create a note on thread A
   - **Target**: `POST /api/threads/{thread_id_A}/notes`
   - **Input**: `{ "content": "Note belongs to thread A." }`
   - **Expected**: 201, returns `ThreadNote` with `thread_id` = `thread_id_A`

2. Attempt to update the note using thread B's URL
   - **Target**: `PUT /api/threads/{thread_id_B}/notes/{note_id}`
   - **Input**: `{ "content": "Trying to move note to thread B." }`
   - **Expected**: 400 Bad Request (thread_id mismatch)

3. Verify the original note is unchanged
   - **Target**: `GET /api/threads/{thread_id_A}/notes`
   - **Input**: `thread_id_A`
   - **Expected**: 200, note content still "Note belongs to thread A."

4. Verify thread B has no notes
   - **Target**: `GET /api/threads/{thread_id_B}/notes`
   - **Input**: `thread_id_B`
   - **Expected**: 200, `{ "notes": [] }`

5. Clean up — delete the note from thread A
   - **Target**: `DELETE /api/threads/{thread_id_A}/notes/{note_id}`
   - **Input**: valid ids
   - **Expected**: 204

## Success Criteria
- [ ] Update with mismatched thread_id returns 400
- [ ] Original note on thread A is unchanged
- [ ] Thread B has no notes (note was not moved)
- [ ] Error response indicates thread_id mismatch

## Failure Criteria
- Update succeeds with mismatched thread_id (note moved between threads)
- Original note content is modified
- Note appears on thread B
- Server returns 500 instead of 400

## Notes
This is a critical authorization boundary test. Notes must be scoped to their thread — a note created on thread A must not be accessible or modifiable via thread B's URL path.
