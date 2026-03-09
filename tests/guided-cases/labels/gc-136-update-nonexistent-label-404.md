# GC-136: Update Non-existent Label Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, validation, update
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Attempt to update a label with a non-existent ID
   - **Target**: PUT /api/labels/00000000-0000-0000-0000-000000000000
   - **Input**: `{"name": "Ghost", "color": "#AAAAAA"}`
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Response status is 404
- [ ] No label named "Ghost" is created as a side effect

## Failure Criteria
- Response status is 200 or 201 (upsert behavior)
- Server error (500)
