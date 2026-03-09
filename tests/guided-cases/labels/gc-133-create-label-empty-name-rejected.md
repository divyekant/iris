# GC-133: Create Label with Empty Name Rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, validation, create
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Attempt to create a label with empty name
   - **Target**: POST /api/labels
   - **Input**: `{"name": "", "color": "#FF0000"}`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Response status is 400
- [ ] No label is created (GET /api/labels unchanged)

## Failure Criteria
- Response status is 201 or 200 (label accepted with empty name)
- Server error (500)
