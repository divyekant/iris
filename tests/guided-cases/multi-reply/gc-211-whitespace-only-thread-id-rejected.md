# GC-211: Whitespace-only thread_id returns 400 Bad Request

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: validation, whitespace, thread-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Send multi-reply request with whitespace-only thread_id (spaces)
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "   " }`
   - **Expected**: 400 Bad Request

2. Send multi-reply request with whitespace-only thread_id (tabs and newlines)
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "\t\n  " }`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Both requests return status 400
- [ ] Neither request triggers an AI generation or DB thread lookup

## Failure Criteria
- Status other than 400 on either request
- Server treats whitespace as a valid thread_id and returns 404 (thread not found) instead of 400

## Notes
The handler uses `.trim().is_empty()` which correctly handles tabs, newlines, and spaces. This case verifies that whitespace-only strings are caught by the same guard as empty strings.
