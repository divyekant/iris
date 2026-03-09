# GC-131: Update Label — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, crud, update
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A label exists (created inline)

## Steps
1. Create a label to update
   - **Target**: POST /api/labels
   - **Input**: `{"name": "BeforeUpdate", "color": "#000000"}`
   - **Expected**: 201 Created, note the returned `id`
2. Update the label with new name and color
   - **Target**: PUT /api/labels/{id}
   - **Input**: `{"name": "AfterUpdate", "color": "#FFFFFF"}`
   - **Expected**: 200 OK with updated name="AfterUpdate" and color="#FFFFFF"

## Success Criteria
- [ ] PUT response status is 200
- [ ] Response body `name` equals "AfterUpdate"
- [ ] Response body `color` equals "#FFFFFF"
- [ ] `id` is unchanged from the created label
- [ ] `created_at` is unchanged from the created label

## Failure Criteria
- PUT returns non-200 status
- Name or color not updated in response
- id or created_at changed unexpectedly
