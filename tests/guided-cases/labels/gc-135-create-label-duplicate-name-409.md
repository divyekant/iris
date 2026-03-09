# GC-135: Create Label with Duplicate Name Returns 409

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, validation, uniqueness
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Create a label
   - **Target**: POST /api/labels
   - **Input**: `{"name": "UniqueTest", "color": "#111111"}`
   - **Expected**: 201 Created
2. Attempt to create another label with the same name
   - **Target**: POST /api/labels
   - **Input**: `{"name": "UniqueTest", "color": "#222222"}`
   - **Expected**: 409 Conflict

## Success Criteria
- [ ] First POST returns 201
- [ ] Second POST returns 409
- [ ] Only one label with name "UniqueTest" exists (verify via GET /api/labels)

## Failure Criteria
- Second POST returns 201 (duplicate name accepted)
- Server error (500) on duplicate
