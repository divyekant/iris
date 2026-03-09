# GC-054: Update Non-Existent Template Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, update, negative, api, 404
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No template with id `99999` exists (or use any sufficiently large id that is guaranteed not to exist)

## Steps

1. Attempt to update a template that does not exist
   - **Target**: `PUT http://localhost:3000/api/templates/99999`
   - **Input**: `{"name": "Ghost Template", "body_text": "This should not work."}`
   - **Expected**: Response status 404. Response body contains an error message indicating the template was not found.

2. Attempt to update with id `0`
   - **Target**: `PUT http://localhost:3000/api/templates/0`
   - **Input**: `{"name": "Zero Template", "body_text": "Edge case id."}`
   - **Expected**: Response status 404.

3. Verify no templates were created as a side effect
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: No template named "Ghost Template" or "Zero Template" appears in the listing.

## Success Criteria
- [ ] PUT with non-existent id returns HTTP 404
- [ ] PUT with id `0` returns HTTP 404
- [ ] No new templates are created as a side effect of the failed updates
- [ ] Response body includes a meaningful error message

## Failure Criteria
- PUT returns HTTP 200 or 201 for a non-existent id
- A new template is created instead of returning 404 (upsert behavior)
- Server returns 500 instead of 404

## Notes
This verifies that the update endpoint strictly requires the template to already exist and does not perform an upsert.
