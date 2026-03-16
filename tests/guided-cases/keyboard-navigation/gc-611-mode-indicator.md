# GC-611: Mode Indicator Shows Current Keyboard Context

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, mode-indicator, ui, context-badge
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app
### Data
- At least one email account connected; inbox has at least one thread

## Steps

1. Navigate to the inbox and verify the mode indicator shows "Inbox"
   - **Target**: http://localhost:3000 (inbox view)
   - **Input**: Load the inbox view
   - **Expected**: A small badge or label in the bottom-left corner of the app reads "Inbox" (or equivalent mode name), styled subtly (low contrast, not distracting)

2. Open a thread and verify the mode indicator changes to "Thread"
   - **Target**: Inbox message list
   - **Input**: Click on any email thread to open it in the thread view
   - **Expected**: The mode indicator updates to "Thread" (or equivalent) while the thread view is active

3. Open the command palette and verify the mode indicator updates
   - **Target**: Iris app (any view)
   - **Input**: Press `Cmd+K` to open the command palette
   - **Expected**: The mode indicator updates to reflect that the command palette is active (e.g., "Command" or "Search"); it reverts to the previous mode when the palette closes

4. Navigate to Settings and verify the mode indicator
   - **Target**: Iris app
   - **Input**: Navigate to the Settings page
   - **Expected**: The mode indicator updates to "Settings" (or equivalent)

5. Return to the inbox and confirm the indicator reverts
   - **Target**: Iris app
   - **Input**: Navigate back to the inbox
   - **Expected**: The mode indicator returns to "Inbox"

## Success Criteria
- [ ] Mode indicator is visible in the bottom-left corner of the app in all views
- [ ] Indicator reads "Inbox" when the inbox is active
- [ ] Indicator reads "Thread" (or equivalent) when a thread is open
- [ ] Indicator reads "Settings" (or equivalent) when Settings is active
- [ ] Indicator updates immediately when the view changes (no stale state)
- [ ] Indicator is subtle and does not obscure other UI elements

## Failure Criteria
- Mode indicator is absent from the bottom-left corner
- Indicator does not update when switching between views
- Indicator shows a stale or incorrect mode label
- Indicator overlaps or obscures interactive UI elements
