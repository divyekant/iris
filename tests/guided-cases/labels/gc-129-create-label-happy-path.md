# GC-129: Create Label — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, crud, create
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Create a new label with valid name and color
   - **Target**: POST /api/labels
   - **Input**: `{"name": "Urgent", "color": "#DC2626"}`
   - **Expected**: 201 Created with JSON body containing id, name="Urgent", color="#DC2626", created_at (integer), message_count=0

## Success Criteria
- [ ] Response status is 201
- [ ] Response body contains a non-empty `id` field (UUID format)
- [ ] `name` equals "Urgent"
- [ ] `color` equals "#DC2626"
- [ ] `created_at` is a positive integer (Unix timestamp)

## Failure Criteria
- Response status is not 201
- Response body is missing id, name, color, or created_at
- Name or color do not match the input values
