# GC-601: Read Message With Only Category Shows No Badge

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: badge-priority
- **Tags**: inbox, message-row, badge, read-state, category
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least one read email classified with category "primary" and no other AI metadata (no needs_reply, no deadline, no non-informational intent, no non-neutral sentiment) (source: local-db)

## Steps

1. Navigate to inbox
   - **Target**: http://localhost:3000/
   - **Expected**: Inbox loads

2. Locate a read message with category "primary" and no other AI flags
   - **Target**: A message row without bold sender name (indicating read)
   - **Expected**: No badges are shown in the badge area. The "primary" category is excluded from badge display since it's the default.

3. Verify visual weight difference between read and unread rows
   - **Target**: Compare read vs unread message rows
   - **Expected**: Read rows have muted sender name (font-weight 400, muted color). Unread rows have bold sender name (font-weight 600, full text color) and a gold left border bar.

## Success Criteria
- [ ] No badge shown for read message with only "primary" category
- [ ] Read message has visually reduced weight (muted text, no gold bar)
- [ ] Visual hierarchy is clear between read and unread rows

## Failure Criteria
- A "primary" category badge is shown
- Read and unread rows have identical visual weight
