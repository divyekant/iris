# GC-191: Boundary — Content at and Beyond 10,000 Character Limit

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: validation, max-length, boundary, create, update
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced email thread exists (source: inbox sync)
- Note the `thread_id` of that thread
- Prepare strings: 10,000 chars (exactly at limit), 10,001 chars (one over limit)

## Steps
1. Create a note with exactly 10,000 characters
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<10,000 character string — e.g., 'A' repeated 10,000 times>" }`
   - **Expected**: 201 with `ThreadNote`, content length is 10,000

2. Verify the full content was stored
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, note content length is exactly 10,000 characters

3. Attempt to create a note with 10,001 characters
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<10,001 character string>" }`
   - **Expected**: 400 Bad Request

4. Attempt to update the existing note to 10,001 characters
   - **Target**: `PUT /api/threads/{thread_id}/notes/{id}`
   - **Input**: `{ "content": "<10,001 character string>" }`
   - **Expected**: 400 Bad Request

5. Verify original note is unchanged
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, note content still exactly 10,000 characters

6. Clean up — delete the note
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{id}`
   - **Input**: valid `thread_id` and `note_id`
   - **Expected**: 204

## Success Criteria
- [ ] 10,000-char content accepted (201)
- [ ] Full 10,000 chars persisted and returned on read
- [ ] 10,001-char content rejected on create (400)
- [ ] 10,001-char content rejected on update (400)
- [ ] Original note unchanged after rejected update

## Failure Criteria
- 10,000-char content is rejected
- Content is silently truncated instead of stored in full
- 10,001-char content is accepted
- Server returns 500 for oversized content

## Notes
Generate test strings programmatically: `"A".repeat(10000)` and `"A".repeat(10001)`. Also verify that a 10,000-char string with trailing whitespace that trims to <= 10,000 is accepted — trimming happens before length check.
