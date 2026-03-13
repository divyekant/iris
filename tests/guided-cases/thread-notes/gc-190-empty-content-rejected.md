# GC-190: Validation — Empty Content Rejected on Create

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: validation, empty-content, create
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
1. Attempt to create a note with empty string content
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "" }`
   - **Expected**: 400 Bad Request

2. Attempt to create a note with whitespace-only content
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "   \n\t  " }`
   - **Expected**: 400 Bad Request (content is trimmed, becomes empty)

3. Attempt to create a note with missing content field
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{}`
   - **Expected**: 400 Bad Request

4. Verify no notes were created
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200 with `{ "notes": [] }`

## Success Criteria
- [ ] Empty string content returns 400
- [ ] Whitespace-only content returns 400 (trimmed to empty)
- [ ] Missing content field returns 400
- [ ] No notes exist after all rejected attempts

## Failure Criteria
- Any empty/whitespace content attempt returns 201
- A note is persisted with empty or whitespace-only content
- Server returns 500 instead of 400

## Notes
Content is trimmed before validation, so whitespace-only strings should be treated as empty.
