# GC-134: Create Label with Empty Color Rejected

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
1. Attempt to create a label with empty color
   - **Target**: POST /api/labels
   - **Input**: `{"name": "NoColor", "color": ""}`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Response status is 400
- [ ] No label named "NoColor" is created

## Failure Criteria
- Response status is 201 or 200 (label accepted with empty color)
- Server error (500)
