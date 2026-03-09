# GC-137: Delete Non-existent Label Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, validation, delete
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Attempt to delete a label with a non-existent ID
   - **Target**: DELETE /api/labels/00000000-0000-0000-0000-000000000000
   - **Input**: none
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Response status is 404

## Failure Criteria
- Response status is 204 (silent success for non-existent resource)
- Server error (500)
