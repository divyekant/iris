# GC-194: Boundary — Multiple Notes on a Single Thread

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: boundary, multiple-notes, ordering
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
1. Create first note
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "Note 1: Initial observations." }`
   - **Expected**: 201, note returned with unique `id`

2. Create second note
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "Note 2: Follow-up thoughts." }`
   - **Expected**: 201, note returned with different `id` than step 1

3. Create third note
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "Note 3: Action items for Monday." }`
   - **Expected**: 201, note returned with unique `id`

4. List all notes
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, exactly 3 notes returned, each with distinct `id` values

5. Verify note ordering
   - **Target**: Inspect the response from step 4
   - **Input**: notes array
   - **Expected**: Notes are ordered by `created_at` (ascending or descending — verify which)

6. Delete the middle note
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{note_2_id}`
   - **Input**: `id` from step 2
   - **Expected**: 204

7. Verify remaining notes are intact
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, exactly 2 notes (note 1 and note 3), content unchanged

8. Clean up — delete remaining notes
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{id}` for each remaining note
   - **Input**: ids from steps 1 and 3
   - **Expected**: 204 for each

## Success Criteria
- [ ] Three notes created with distinct IDs
- [ ] List returns all 3 notes
- [ ] Notes have consistent ordering
- [ ] Deleting one note does not affect others
- [ ] Remaining notes have unchanged content after sibling deletion

## Failure Criteria
- Duplicate note IDs returned
- List returns wrong count
- Deleting one note corrupts or removes other notes
- Note content changes after sibling deletion

## Notes
Pay attention to the ordering of notes in the list response — document whether it is ascending or descending by `created_at`. This establishes the expected ordering contract for the UI.
