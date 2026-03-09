# GC-130: List Labels Returns Created Label

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, crud, list
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- At least one label created (from GC-129 or inline setup)

## Steps
1. Create a label for listing
   - **Target**: POST /api/labels
   - **Input**: `{"name": "ListTest", "color": "#16A34A"}`
   - **Expected**: 201 Created
2. List all labels
   - **Target**: GET /api/labels
   - **Input**: none
   - **Expected**: 200 OK with JSON array containing the created label with id, name="ListTest", color="#16A34A", created_at, and message_count=0

## Success Criteria
- [ ] GET /api/labels returns 200
- [ ] Response is a JSON array
- [ ] Array contains an entry with name="ListTest"
- [ ] That entry includes `message_count` field equal to 0

## Failure Criteria
- GET returns non-200 status
- Response is not an array
- Created label is missing from the list
- `message_count` field is absent
