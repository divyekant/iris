# GC-198: Validation — Update and Delete with Missing or Empty Note ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: validation, note-id, empty, update, delete
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
1. Attempt to update a note with empty note_id in the URL
   - **Target**: `PUT /api/threads/{thread_id}/notes/`
   - **Input**: `{ "content": "This should fail." }`
   - **Expected**: 400 Bad Request or 404 Not Found (route mismatch)

2. Attempt to delete a note with empty note_id in the URL
   - **Target**: `DELETE /api/threads/{thread_id}/notes/`
   - **Input**: empty note_id path segment
   - **Expected**: 400 Bad Request or 404 Not Found (route mismatch)

3. Attempt to update a note with non-numeric note_id
   - **Target**: `PUT /api/threads/{thread_id}/notes/not-a-number`
   - **Input**: `{ "content": "Invalid ID format." }`
   - **Expected**: 400 Bad Request (invalid note_id format)

4. Attempt to delete a note with non-numeric note_id
   - **Target**: `DELETE /api/threads/{thread_id}/notes/not-a-number`
   - **Input**: non-numeric note_id
   - **Expected**: 400 Bad Request (invalid note_id format)

## Success Criteria
- [ ] Empty note_id on update returns 400 or 404
- [ ] Empty note_id on delete returns 400 or 404
- [ ] Non-numeric note_id on update returns 400
- [ ] Non-numeric note_id on delete returns 400
- [ ] No 500 errors for any malformed note_id

## Failure Criteria
- Server returns 500 for empty or invalid note_id
- Request with empty note_id is routed to the list endpoint and succeeds unexpectedly
- Non-numeric note_id causes a panic or unhandled error

## Notes
The router may handle empty path segments differently from malformed ones. Empty note_id might match a different route (e.g., the list endpoint for GET, or nothing for PUT/DELETE). The key validation is that no mutation occurs and the server responds with a client error code.
