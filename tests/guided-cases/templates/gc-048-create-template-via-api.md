# GC-048: Create a Template via API

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, create, api, crud
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No templates need to exist beforehand

## Steps

1. Send POST request to create a template
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Meeting Follow-Up", "subject": "Following up on our meeting", "body_text": "Hi,\n\nThank you for taking the time to meet today. Here are the action items we discussed:\n\n1. ...\n2. ...\n\nBest regards", "body_html": "<p>Hi,</p><p>Thank you for taking the time to meet today. Here are the action items we discussed:</p><ol><li>...</li><li>...</li></ol><p>Best regards</p>"}`
   - **Expected**: Response status 201. Response body contains `id`, `name`, `subject`, `body_text`, `body_html`, `created_at`, and `updated_at` fields. The `name` matches "Meeting Follow-Up" and `subject` matches "Following up on our meeting".

2. Verify the template persists by listing
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: Response contains an array with at least one entry whose `id` matches the one returned in step 1 and whose `name` is "Meeting Follow-Up".

## Success Criteria
- [ ] POST returns HTTP 201
- [ ] Response body includes all expected fields: id, name, subject, body_text, body_html, created_at, updated_at
- [ ] Field values match the input payload
- [ ] Template appears in subsequent GET /api/templates listing

## Failure Criteria
- POST returns a non-201 status code
- Response body is missing required fields (e.g., no `id` or no `created_at`)
- Template does not appear in the list after creation

## Notes
This is the foundational CRUD test. The created template will be used as a prerequisite for GC-049, GC-050, and GC-051 if run sequentially.
