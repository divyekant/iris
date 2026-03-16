# GC-604: Organize Dropdown Contains Star/Snooze/Archive/Delete

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: grouped-action-bar
- **Tags**: thread-view, action-bar, dropdown, organize, keyboard
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least one email thread (source: local-db)

## Steps

1. Open a thread
   - **Target**: Click any thread from inbox
   - **Expected**: ThreadView loads

2. Click the "Organize" dropdown trigger
   - **Target**: "Organize" button with chevron in the action bar
   - **Expected**: Dropdown menu opens with scale animation showing: Star (shortcut: s), Snooze (shortcut: b), Archive (shortcut: e), Delete (shortcut: #)

3. Navigate dropdown with arrow keys
   - **Target**: Opened dropdown menu
   - **Input**: Press ArrowDown 3 times, then ArrowUp once
   - **Expected**: Focus indicator moves down through items, then back up. Each focused item has highlighted background.

4. Press Escape to close
   - **Target**: Opened dropdown
   - **Input**: Press Escape key
   - **Expected**: Dropdown closes with animation

5. Test keyboard shortcut bypasses dropdown
   - **Target**: Thread view (dropdown closed)
   - **Input**: Press "e" key
   - **Expected**: Thread is archived directly without opening the dropdown. Keyboard shortcuts work independently of the dropdown UI.

## Success Criteria
- [ ] Organize dropdown contains exactly: Star, Snooze, Archive, Delete
- [ ] Each item shows its keyboard shortcut
- [ ] Arrow key navigation works within the dropdown
- [ ] Escape closes the dropdown
- [ ] Direct keyboard shortcuts (e, s, #, b) work without opening dropdown

## Failure Criteria
- Dropdown is missing any of the 4 items
- Keyboard navigation does not work
- Keyboard shortcuts require opening dropdown first
