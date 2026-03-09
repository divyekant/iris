# GC-057: Empty Template List Shows "No templates yet" in Picker

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: ui
- **Flow**: templates
- **Tags**: templates, empty-state, ui, picker, edge
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Frontend accessible in a browser at http://localhost:3000

### Data
- **No templates exist.** Delete all templates via API if any are present: for each template in `GET /api/templates`, call `DELETE /api/templates/{id}`.

## Steps

1. Verify template list is empty via API
   - **Target**: `GET http://localhost:3000/api/templates`
   - **Expected**: Response status 200. Response body is an empty array `[]`.

2. Open the Compose modal
   - **Target**: Click the "Compose" button in the Iris UI
   - **Expected**: The ComposeModal opens with empty fields.

3. Open the TemplatePicker dropdown
   - **Target**: Click the template picker button/dropdown inside the ComposeModal
   - **Expected**: The dropdown opens and displays a "No templates yet" message (or similar empty-state text) instead of a list of templates.

4. Verify the "Manage Templates..." link is still visible
   - **Target**: Look for a "Manage Templates..." link or button in the TemplatePicker dropdown
   - **Expected**: The link is present even when no templates exist, allowing the user to navigate to Settings to create their first template.

5. Click "Manage Templates..." link
   - **Target**: Click the "Manage Templates..." link in the TemplatePicker
   - **Expected**: The browser navigates to the Settings page (or the Templates section within Settings). The ComposeModal may close or remain open depending on implementation.

## Success Criteria
- [ ] API returns an empty array when no templates exist
- [ ] TemplatePicker dropdown shows an empty-state message (e.g., "No templates yet")
- [ ] No error or blank dropdown is shown
- [ ] "Manage Templates..." link is visible and functional
- [ ] Clicking "Manage Templates..." navigates to Settings

## Failure Criteria
- TemplatePicker dropdown shows an error when template list is empty
- Dropdown appears completely blank with no guidance
- "Manage Templates..." link is missing in the empty state
- JavaScript errors appear in the browser console

## Notes
The exact wording of the empty-state message may vary ("No templates yet", "No templates", "Create your first template", etc.). Record the actual text. The key requirement is that the user gets clear feedback and a path to create templates.
