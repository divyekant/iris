# GC-599: Message Row Shows "Needs Reply" as Primary Badge

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: badge-priority
- **Tags**: inbox, message-row, badge, needs-reply, visual-hierarchy
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- At least one email account synced

### Data
- At least one email flagged as needs_reply by AI classification (source: local-db, setup: AI classification runs automatically on synced messages)
- The needs_reply message should also have an intent and category set (source: local-db)

## Steps

1. Navigate to inbox
   - **Target**: http://localhost:3000/
   - **Expected**: Inbox loads with message list

2. Locate a message row that has been AI-classified as needs_reply
   - **Target**: Any message row with a badge displayed
   - **Expected**: The message row shows exactly ONE badge — "Needs Reply" in a warning-colored pill (gold/amber background)

3. Verify no other badges are visible inline on the same row
   - **Target**: The badge area of the needs_reply message row
   - **Expected**: Only the "Needs Reply" badge is visible. If the message also has intent, sentiment, or category metadata, these are NOT shown as inline badges.

4. Check for overflow indicator
   - **Target**: Next to the "Needs Reply" badge
   - **Expected**: If the message has additional AI metadata (intent, sentiment, category), a "+N" chip appears next to the badge showing how many additional badges are hidden. If only needs_reply is set, no overflow chip appears.

## Success Criteria
- [ ] Exactly one primary badge is shown on the message row
- [ ] The badge text reads "Needs Reply"
- [ ] The badge uses the warning color variant
- [ ] Overflow count is accurate (matches number of additional AI metadata fields present)

## Failure Criteria
- Multiple badges are displayed inline on the same row
- No badge is shown despite the message being flagged as needs_reply
- Badge uses wrong color (not warning)
