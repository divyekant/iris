# GC-056: Pick Template from TemplatePicker Fills Compose Fields

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: templates
- **Tags**: templates, compose, ui, picker, integration
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Frontend accessible in a browser at http://localhost:3000

### Data
- At least one template exists with both `subject` and `body_text` populated (source: create via `POST /api/templates` or Settings UI). Example: name="Meeting Follow-Up", subject="Following up on our meeting", body_text="Hi,\n\nThank you for taking the time to meet today..."

## Steps

1. Create a template via API (if not already present)
   - **Target**: `POST http://localhost:3000/api/templates`
   - **Input**: `{"name": "Meeting Follow-Up", "subject": "Following up on our meeting", "body_text": "Hi,\n\nThank you for taking the time to meet today.\n\nBest regards"}`
   - **Expected**: Response status 201.

2. Open the Compose modal
   - **Target**: Click the "Compose" button in the Iris UI (top nav or inbox view)
   - **Expected**: The ComposeModal opens with empty To, Subject, and Body fields.

3. Open the TemplatePicker dropdown
   - **Target**: Click the template picker button/dropdown inside the ComposeModal
   - **Expected**: A dropdown appears listing available templates. "Meeting Follow-Up" is visible in the list.

4. Select the "Meeting Follow-Up" template
   - **Target**: Click "Meeting Follow-Up" in the TemplatePicker dropdown
   - **Expected**: The Subject field is filled with "Following up on our meeting". The Body field is filled with "Hi,\n\nThank you for taking the time to meet today.\n\nBest regards". The To field remains unchanged (empty or whatever was previously entered).

5. Verify fields are editable after template insertion
   - **Target**: Click into the Subject field and append " - Monday"
   - **Expected**: The Subject field now reads "Following up on our meeting - Monday". The body remains unchanged.

## Success Criteria
- [ ] TemplatePicker dropdown shows the created template by name
- [ ] Selecting a template fills the Subject field with the template's subject
- [ ] Selecting a template fills the Body field with the template's body_text
- [ ] The To field is not overwritten by template selection
- [ ] Filled fields remain editable after template insertion

## Failure Criteria
- TemplatePicker dropdown is empty despite templates existing
- Selecting a template does not fill Subject or Body
- Template selection overwrites the To field
- Fields become read-only after template insertion
- JavaScript errors appear in the browser console during template selection

## Notes
If the compose body uses a rich-text editor, the body_html (if present on the template) should be used. If body_html is null, body_text should be rendered as plain text. Verify which format is applied based on the template data.
