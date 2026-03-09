# GC-132: Delete Label — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, crud, delete
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A label exists (created inline)

## Steps
1. Create a label to delete
   - **Target**: POST /api/labels
   - **Input**: `{"name": "ToDelete", "color": "#FF0000"}`
   - **Expected**: 201 Created, note the returned `id`
2. Delete the label
   - **Target**: DELETE /api/labels/{id}
   - **Input**: none
   - **Expected**: 204 No Content with empty body
3. Verify label is gone
   - **Target**: GET /api/labels
   - **Input**: none
   - **Expected**: 200 OK, array does NOT contain label with name="ToDelete"

## Success Criteria
- [ ] DELETE returns 204
- [ ] DELETE response body is empty
- [ ] Subsequent GET /api/labels does not include the deleted label

## Failure Criteria
- DELETE returns non-204 status
- Deleted label still appears in GET /api/labels response
