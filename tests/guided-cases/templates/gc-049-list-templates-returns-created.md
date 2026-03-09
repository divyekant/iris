# GC-049: List Templates Returns Created Template

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: templates
- **Tags**: templates, list, api, crud
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- At least one template created via `POST /api/templates` (source: GC-048 or manual setup)

## Steps

1. Create two templates to ensure multiple entries
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: Template A: `{"name": "Welcome Email", "body_text": "Welcome to the team! We are glad to have you."}`
   - **Expected**: Response status 201. Note the returned `id` as `id_a`.

2. Create a second template
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: Template B: `{"name": "Invoice Reminder", "subject": "Invoice #{{number}} is overdue", "body_text": "This is a friendly reminder that invoice #{{number}} is now overdue."}`
   - **Expected**: Response status 201. Note the returned `id` as `id_b`.

3. List all templates
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: Response status 200. Response body is a JSON array containing at least 2 entries. Both `id_a` and `id_b` appear in the array with their correct names and body_text.

## Success Criteria
- [ ] GET returns HTTP 200
- [ ] Response body is a JSON array
- [ ] Both created templates appear in the array
- [ ] Each template object contains all fields: id, name, subject, body_text, body_html, created_at, updated_at
- [ ] Templates without an explicit subject have subject as null or empty string

## Failure Criteria
- GET returns a non-200 status code
- Response is not a JSON array
- One or both created templates are missing from the list
- Template fields are missing or have incorrect values

## Notes
The `subject` field is optional on creation. Template A omits it; verify the list response handles that gracefully (null or empty, not an error).
