# GC-602: Message Row Animates Out on Archive

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: badge-priority
- **Tags**: inbox, message-row, animation, archive, collapse
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least 3 messages in inbox (source: local-db)

## Steps

1. Navigate to inbox
   - **Target**: http://localhost:3000/
   - **Expected**: Inbox loads with multiple message rows

2. Hover over a message row to reveal action buttons
   - **Target**: Any message row in the inbox
   - **Expected**: 4 action buttons appear on the right side (Archive, Delete, Star, Snooze)

3. Click the Archive button
   - **Target**: Archive button on the hovered row
   - **Expected**: The message row collapses smoothly (height animates to 0, opacity fades) rather than disappearing instantly. The list reflows to fill the gap.

4. Verify toast feedback appears
   - **Target**: Bottom-right corner of the screen
   - **Expected**: A toast notification appears with "Archived" text and an "Undo" button. Toast slides in with animation.

## Success Criteria
- [ ] Row exits with a smooth collapse animation (not instant removal)
- [ ] Remaining rows reflow to fill the gap
- [ ] Toast notification appears with undo option
- [ ] Toast has slide-in animation

## Failure Criteria
- Row disappears instantly without animation
- List does not reflow (gap remains)
- No toast notification appears
