# GC-065: Report Spam via SpamDialog UI

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: spam-block
- **Tags**: spam, block-sender, spam-dialog, ui, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Browser open to the Iris inbox at http://localhost:3000/#/

### Data
- At least one message visible in the inbox (source: IMAP sync)
- Note the sender email of the target message

## Steps

1. Hover over a message row to reveal quick actions
   - **Target**: MessageRow component in Inbox
   - **Expected**: Quick action icons appear, including a ShieldAlert (spam report) icon button

2. Click the ShieldAlert icon to open the SpamDialog
   - **Target**: ShieldAlert icon button on the hovered MessageRow
   - **Expected**: SpamDialog modal appears showing the sender email, message count ("1 message"), and an "Also block this sender" checkbox that is checked by default

3. Verify the SpamDialog content
   - **Target**: SpamDialog.svelte modal
   - **Expected**: Sender email is displayed correctly; message count is accurate; "Also block this sender" checkbox is checked; Confirm and Cancel buttons are visible

4. Click the Confirm button
   - **Target**: Confirm button in SpamDialog
   - **Expected**: Dialog closes; the message disappears from the inbox; a success notification or visual feedback appears

5. Verify the message moved to spam
   - **Target**: Navigate to spam folder or check `GET /api/messages?folder=spam`
   - **Expected**: The reported message appears in the spam folder

6. Verify the sender was blocked
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: The sender's email address appears in the blocked senders list (since the checkbox was checked by default)

## Success Criteria
- [ ] ShieldAlert icon appears on hover over MessageRow
- [ ] SpamDialog opens with correct sender email and message count
- [ ] "Also block this sender" checkbox is checked by default
- [ ] Clicking Confirm closes the dialog and removes the message from inbox
- [ ] Message appears in spam folder after confirmation
- [ ] Sender appears in blocked-senders list
- [ ] No console errors during the flow

## Failure Criteria
- ShieldAlert icon does not appear or is not clickable
- SpamDialog does not open or shows incorrect information
- Checkbox is unchecked by default
- Message remains in inbox after confirming
- Sender is not blocked despite checkbox being checked
- JavaScript errors in browser console

## Notes
This is the primary end-to-end UI test for the spam report flow. It exercises MessageRow quick actions, SpamDialog rendering and interaction, and the underlying report-spam API call with block_sender=true.
