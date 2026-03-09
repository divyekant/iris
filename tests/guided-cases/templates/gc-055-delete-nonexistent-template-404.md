# GC-055: Delete Non-Existent Template Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, delete, negative, api, 404
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No template with id `99999` exists

## Steps

1. Attempt to delete a template that does not exist
   - **Target**: `DELETE http://localhost:3000/api/templates/99999`
   - **Expected**: Response status 404. Response body contains an error message indicating the template was not found.

2. Attempt to delete with id `0`
   - **Target**: `DELETE http://localhost:3000/api/templates/0`
   - **Expected**: Response status 404.

3. Attempt to delete with a negative id
   - **Target**: `DELETE http://localhost:3000/api/templates/-1`
   - **Expected**: Response status 404 (or 400 if the server rejects negative ids). Should not return 204.

## Success Criteria
- [ ] DELETE with non-existent id returns HTTP 404
- [ ] DELETE with id `0` returns HTTP 404
- [ ] DELETE with negative id returns HTTP 404 or 400 (not 204)
- [ ] No side effects on existing templates

## Failure Criteria
- DELETE returns HTTP 204 for a non-existent id (false success)
- Server returns 500 for any of the requests
- An existing template is accidentally deleted

## Notes
Step 3 tests boundary id values. Record actual behavior for negative ids; if the route parser rejects them before reaching the handler, the status may differ (e.g., 400 or 422).
