# GC-196: Negative — Delete Nonexistent Note

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: negative, delete, not-found
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced email thread exists (source: inbox sync)
- Note the `thread_id` of that thread

## Steps
1. Attempt to delete a note with a fabricated ID
   - **Target**: `DELETE /api/threads/{thread_id}/notes/99999999`
   - **Input**: valid `thread_id`, nonexistent `note_id` = `99999999`
   - **Expected**: 404 Not Found

2. Create a note, then delete it
   - **Target**: `POST /api/threads/{thread_id}/notes` then `DELETE /api/threads/{thread_id}/notes/{id}`
   - **Input**: `{ "content": "Ephemeral note." }` → then delete
   - **Expected**: 201 then 204

3. Attempt to delete the same note again (already deleted)
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{id}`
   - **Input**: the `id` from step 2 (already deleted)
   - **Expected**: 404 Not Found

4. Attempt to update the deleted note
   - **Target**: `PUT /api/threads/{thread_id}/notes/{id}`
   - **Input**: `{ "content": "Ghost update." }`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Fabricated note ID returns 404 on delete
- [ ] Already-deleted note returns 404 on second delete
- [ ] Already-deleted note returns 404 on update attempt
- [ ] No 500 errors for any nonexistent note operation

## Failure Criteria
- Delete of nonexistent note returns 200 or 204
- Double-delete returns 204 (idempotency without 404)
- Update of deleted note succeeds
- Server returns 500

## Notes
The API specifies 404 for deleting nonexistent notes. This differs from some REST APIs that treat DELETE as idempotent (always 204). Verify the 404 behavior matches the spec.
