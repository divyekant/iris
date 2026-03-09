# GC-050: Update a Template via API

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, update, api, crud
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- One template already created (source: `POST /api/templates` or GC-048). Note its `id` and original `updated_at` timestamp.

## Steps

1. Create a template to update
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Draft Response", "subject": "Re: Your inquiry", "body_text": "Thank you for reaching out. We will get back to you shortly."}`
   - **Expected**: Response status 201. Record the `id` and `updated_at` values.

2. Update the template name, subject, and body
   - **Target**: `PUT http://localhost:3000/api/templates/{id}` (using the id from step 1)
   - **Input**: `{"name": "Detailed Response", "subject": "Re: Your detailed inquiry", "body_text": "Thank you for reaching out. Here is a detailed response to your questions:\n\n1. ...\n2. ...\n\nPlease let us know if you have further questions."}`
   - **Expected**: Response status 200. Response body shows the updated `name`, `subject`, and `body_text`. The `updated_at` timestamp is equal to or later than the original.

3. Verify the update persisted
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: The template with the given `id` now has `name` "Detailed Response" and the updated body text.

## Success Criteria
- [ ] PUT returns HTTP 200
- [ ] Response body reflects the updated name, subject, and body_text
- [ ] The `updated_at` timestamp is equal to or later than the original `updated_at`
- [ ] GET listing confirms the changes persisted
- [ ] The `id` and `created_at` remain unchanged

## Failure Criteria
- PUT returns a non-200 status code
- Response body still shows original values
- GET listing does not reflect the update
- The `id` or `created_at` changed after the update

## Notes
Updating a template should not change its `id` or `created_at`. Only `name`, `subject`, `body_text`, `body_html`, and `updated_at` should change.
