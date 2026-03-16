# GC-600: Message Row Shows Deadline as Primary Badge When No Needs-Reply

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: badge-priority
- **Tags**: inbox, message-row, badge, deadline, priority-order
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- At least one email account synced

### Data
- At least one email with a deadline extracted but NOT flagged as needs_reply (source: local-db, setup: AI deadline extraction runs on synced messages)

## Steps

1. Navigate to inbox
   - **Target**: http://localhost:3000/
   - **Expected**: Inbox loads with message list

2. Locate a message with a deadline but no needs_reply flag
   - **Target**: Message row with a red/error-colored badge
   - **Expected**: The badge shows "Due [date]" (e.g., "Due Mar 20") in error color (red)

3. Verify the deadline badge takes priority over intent/sentiment/category
   - **Target**: Badge area of the message row
   - **Expected**: Only the deadline badge is visible as the primary badge. Intent, sentiment, and category are hidden.

## Success Criteria
- [ ] Deadline badge is shown as the primary (only visible) badge
- [ ] Badge text includes the deadline date
- [ ] Badge uses the error color variant (red)
- [ ] Lower-priority badges (intent, sentiment, category) are not shown inline

## Failure Criteria
- Intent or category badge is shown instead of deadline
- Multiple badges are shown inline
- Deadline badge is missing despite the message having a deadline
