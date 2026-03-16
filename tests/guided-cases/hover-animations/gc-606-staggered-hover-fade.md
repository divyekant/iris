# GC-606: Message Row Hover Actions Appear with Staggered Fade

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: hover-animations
- **Tags**: inbox, message-row, hover, animation, staggered-fade
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least 2 messages in inbox (source: local-db)

## Steps

1. Navigate to inbox
   - **Target**: http://localhost:3000/
   - **Expected**: Inbox loads with message rows

2. Hover over a message row
   - **Target**: Any message row
   - **Expected**: 4 action buttons (Archive, Delete, Star, Snooze) appear on the right side with a staggered fade-in animation — each button appears slightly after the previous one (30ms stagger). The buttons overlay the date area, not the subject.

3. Move mouse away from the row
   - **Target**: Move cursor to empty space or another row
   - **Expected**: Action buttons fade out smoothly

4. Hover over a different row
   - **Target**: A different message row
   - **Expected**: Actions appear on the new row with the same staggered animation. Actions on the previous row are gone.

## Success Criteria
- [ ] 4 action buttons appear on hover (Archive, Delete, Star, Snooze)
- [ ] Buttons appear with a visible staggered animation (not all at once)
- [ ] Buttons are positioned over the date/right area, not over subject text
- [ ] Buttons fade out when hover ends

## Failure Criteria
- All buttons appear simultaneously (no stagger effect)
- Buttons appear over the subject or sender text
- Buttons remain visible after mouse leaves the row
