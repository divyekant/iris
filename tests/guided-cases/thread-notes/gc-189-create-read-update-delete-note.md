# GC-189: CRUD Happy Path — Create, Read, Update, Delete a Thread Note

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: crud, happy-path, create, read, update, delete
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
1. List notes for the thread (expect empty)
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200 with `{ "notes": [] }`

2. Create a note on the thread
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "Follow up with Alex about the Q3 budget review." }`
   - **Expected**: 201 with `ThreadNote` containing `id`, `thread_id`, `content` matching input, `created_at`, `updated_at`

3. List notes again to confirm creation
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200 with `{ "notes": [...] }` containing exactly 1 note matching the created note

4. Update the note content
   - **Target**: `PUT /api/threads/{thread_id}/notes/{id}`
   - **Input**: `{ "content": "Follow up with Alex about the Q3 budget review. Deadline is Friday." }`
   - **Expected**: 200 with updated `ThreadNote`, `content` matches new value, `updated_at` >= `created_at`

5. Verify update persisted
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, single note with updated content

6. Delete the note
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{id}`
   - **Input**: valid `thread_id` and `note_id`
   - **Expected**: 204 No Content

7. Verify deletion
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200 with `{ "notes": [] }`

## Success Criteria
- [ ] Step 1 returns empty notes array
- [ ] Step 2 returns 201 with all ThreadNote fields populated
- [ ] Step 3 returns exactly 1 note matching the created note
- [ ] Step 4 returns updated content and `updated_at` >= `created_at`
- [ ] Step 5 confirms the update persisted
- [ ] Step 6 returns 204
- [ ] Step 7 returns empty notes array

## Failure Criteria
- Any step returns an unexpected status code
- Created note is missing `id`, `thread_id`, `created_at`, or `updated_at`
- Updated note content does not match the new value
- Note still appears in list after deletion

## Notes
This is the core CRUD lifecycle test. All other cases depend on this flow working correctly.
