# GC-053: Create Template with Empty body_text Rejected

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

1. Send POST with an empty-string body_text
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Valid Name", "body_text": ""}`
   - **Expected**: Response status 400. Response body contains `{"error": "body_text is required"}`.

2. Send POST with the body_text field omitted entirely
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Valid Name"}`
   - **Expected**: Response status 400. Response body contains an error message indicating body_text is required.

3. Send POST with a whitespace-only body_text
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Valid Name", "body_text": "   "}`
   - **Expected**: Response status 400. Response body contains an error message indicating body_text is required (if whitespace-only is treated as empty).

4. Verify no templates were created from the invalid requests
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: No templates named "Valid Name" with empty body text appear in the listing.

## Success Criteria
- [ ] Empty-string body_text returns HTTP 400
- [ ] Error message is `"body_text is required"` (or equivalent)
- [ ] Missing body_text field returns HTTP 400
- [ ] No invalid templates are persisted

## Failure Criteria
- Any of the invalid requests returns HTTP 201
- A template with an empty or missing body_text is persisted in the database
- Error response does not include a meaningful error message

## Notes
Step 3 (whitespace-only) tests trim-before-validate behavior. Record actual outcome. The `body_html` field is optional and should not be validated as required.
