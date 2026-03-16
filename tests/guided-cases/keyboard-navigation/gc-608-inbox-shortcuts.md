# GC-608: Inbox Keyboard Shortcuts Navigate and Act on Messages

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, shortcuts, inbox, j-k-navigation, archive, star, delete, centralized-manager
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app window (not an input field)
### Data
- At least 3 emails in the inbox (source: local-db)
- At least one email is unstarred and unarchived

## Steps

1. Navigate to the inbox and confirm the first message is focused
   - **Target**: http://localhost:3000
   - **Input**: Navigate to the inbox view
   - **Expected**: Inbox loads; the first message row is visually highlighted/focused (keyboard focus indicator visible)

2. Press `j` to move focus to the next message
   - **Target**: Inbox message list
   - **Input**: Press `j`
   - **Expected**: Focus moves down to the second message row; visual focus indicator shifts accordingly

3. Press `k` to move focus back to the previous message
   - **Target**: Inbox message list
   - **Input**: Press `k`
   - **Expected**: Focus moves back up to the first message row

4. Press `e` to archive the focused message
   - **Target**: Focused message row
   - **Input**: Press `e`
   - **Expected**: The focused message is archived and removed from the inbox view; focus moves to the next message automatically

5. Press `s` to star a message
   - **Target**: Focused message row
   - **Input**: Press `s`
   - **Expected**: The star icon on the focused message toggles on (starred); the message remains in the inbox

6. Press `s` again to unstar the message
   - **Target**: Same focused message
   - **Input**: Press `s`
   - **Expected**: The star icon toggles off (unstarred)

7. Press `#` to delete the focused message
   - **Target**: Focused message row
   - **Input**: Press `#`
   - **Expected**: The focused message is moved to trash and removed from the inbox view; focus moves to the next message

## Success Criteria
- [ ] `j` moves keyboard focus to the next message in the list
- [ ] `k` moves keyboard focus to the previous message in the list
- [ ] `e` archives the focused message and removes it from the inbox
- [ ] `s` toggles the starred state of the focused message
- [ ] `#` deletes (moves to trash) the focused message
- [ ] All shortcuts are registered through the centralized keyboard manager (`keyboard.ts`) rather than inline event listeners
- [ ] Shortcuts do not trigger when focus is inside an input, textarea, or contenteditable element

## Failure Criteria
- Any shortcut key has no effect on the focused message
- `j`/`k` navigation cycles incorrectly or skips messages
- Shortcuts fire while the user is typing in a search box or compose field
- Archive or delete actions do not remove the message from the inbox view
