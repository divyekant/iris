# GC-052: Create Template with Empty Name Rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, validation, negative, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No specific data prerequisites

## Steps

1. Send POST with an empty-string name
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "", "body_text": "Some valid body text."}`
   - **Expected**: Response status 400. Response body contains `{"error": "name is required"}`.

2. Send POST with the name field omitted entirely
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"body_text": "Some valid body text."}`
   - **Expected**: Response status 400. Response body contains an error message indicating name is required.

3. Send POST with a whitespace-only name
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "   ", "body_text": "Some valid body text."}`
   - **Expected**: Response status 400. Response body contains an error message indicating name is required (if whitespace-only is treated as empty).

4. Verify no templates were created from the invalid requests
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: No templates with empty or whitespace-only names appear in the listing.

## Success Criteria
- [ ] Empty-string name returns HTTP 400
- [ ] Error message is `"name is required"` (or equivalent)
- [ ] Missing name field returns HTTP 400
- [ ] No invalid templates are persisted

## Failure Criteria
- Any of the invalid requests returns HTTP 201
- A template with an empty or missing name is persisted in the database
- Error response does not include a meaningful error message

## Notes
Step 3 (whitespace-only) may or may not be rejected depending on whether the server trims names before validation. Record the actual behavior. If whitespace-only names are accepted, note this as a potential hardening item.
