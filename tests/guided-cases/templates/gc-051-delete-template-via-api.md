# GC-051: Delete a Template via API

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, delete, api, crud
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- One template already created (source: `POST /api/templates` or GC-048). Note its `id`.

## Steps

1. Create a template to delete
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Temporary Template", "body_text": "This template will be deleted."}`
   - **Expected**: Response status 201. Record the `id`.

2. Delete the template
   - **Target**: `DELETE http://localhost:3000/api/templates/{id}` (using the id from step 1)
   - **Expected**: Response status 204 (No Content). Response body is empty.

3. Verify the template no longer exists in the list
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: The template with the deleted `id` does not appear in the returned array.

4. Attempt to delete the same template again
   - **Target**: `DELETE http://localhost:3000/api/templates/{id}` (same id)
   - **Expected**: Response status 404. The template no longer exists.

## Success Criteria
- [ ] DELETE returns HTTP 204
- [ ] Response body is empty on successful deletion
- [ ] Template no longer appears in GET /api/templates listing
- [ ] Repeat DELETE on same id returns HTTP 404

## Failure Criteria
- DELETE returns a status other than 204
- Template still appears in the list after deletion
- Repeat DELETE does not return 404

## Notes
Step 4 doubles as a sanity check that overlaps with GC-055 (delete non-existent). If running in isolation, step 4 alone confirms idempotency behavior.
