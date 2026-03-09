# gc-rtc-005: Rich Text Editor Toolbar Displays

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: rich-text-compose
- **Tags**: tiptap, toolbar, compose, editor, ui
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://127.0.0.1:3000
- Browser access to the web UI
- At least one email account configured

### Data
- None required (source: inline)

## Steps

1. Navigate to the Iris app
   - **Target**: http://127.0.0.1:3000
   - **Expected**: App loads with TopNav visible

2. Open the compose modal
   - **Target**: Click the "Compose" button in the navigation/toolbar
   - **Expected**: ComposeModal opens with the TipTap rich text editor visible

3. Verify the formatting toolbar is present
   - **Target**: Inspect the toolbar area above the editor content area
   - **Expected**: The following formatting controls are visible:
     - **Bold** button (B)
     - **Italic** button (I)
     - **Underline** button (U)
     - **Strikethrough** button (S)
     - **Font family** dropdown
     - **Font size** dropdown
     - **Text color** picker
     - **Highlight** picker
     - **Bullet list** button
     - **Ordered list** button
     - **Link** button
     - **Clear formatting** button

4. Verify the editor accepts text input
   - **Target**: Click into the editor content area and type "Test message"
   - **Expected**: Text appears in the editor area; cursor is blinking

5. Apply bold formatting
   - **Target**: Select the word "Test" and click the Bold button
   - **Expected**: The word "Test" becomes bold; the Bold button shows as active/toggled

## Success Criteria
- [ ] ComposeModal opens with TipTap editor visible
- [ ] All 12 formatting toolbar controls are present and visible
- [ ] Editor accepts text input
- [ ] Bold formatting can be applied and the button state reflects the selection
- [ ] Editor content area has appropriate height and is scrollable

## Failure Criteria
- ComposeModal opens but shows a plain textarea instead of TipTap editor
- One or more toolbar buttons are missing
- Editor does not accept text input
- Formatting buttons do not respond to clicks
- Note: This test will be SKIPPED during automated execution (no browser access)
