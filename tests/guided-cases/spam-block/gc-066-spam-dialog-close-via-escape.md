# GC-066: SpamDialog Close via Escape Key

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: ui
- **Flow**: spam-block
- **Tags**: spam-dialog, keyboard, escape, edge, ui
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Browser open to the Iris inbox at http://localhost:3000/#/

### Data
- At least one message visible in the inbox (source: IMAP sync)

## Steps

1. Hover over a message row and click the ShieldAlert icon
   - **Target**: ShieldAlert icon button on MessageRow
   - **Expected**: SpamDialog modal opens

2. Press the Escape key
   - **Target**: Keyboard input while SpamDialog is open
   - **Expected**: SpamDialog closes immediately without any API call

3. Verify the message is unchanged
   - **Target**: Inbox view
   - **Expected**: The message remains in the inbox in its original position and state

4. Verify no spam report was submitted
   - **Target**: Browser Network tab or `GET /api/messages?folder=spam`
   - **Expected**: No POST to /api/messages/report-spam was made; message is not in spam folder

5. Re-open the SpamDialog and close via Cancel button
   - **Target**: ShieldAlert icon, then Cancel button in SpamDialog
   - **Expected**: Dialog opens and closes cleanly via Cancel; message remains in inbox

## Success Criteria
- [ ] Escape key closes the SpamDialog
- [ ] No API call is made when dialog is dismissed via Escape
- [ ] Message remains in inbox unchanged
- [ ] Cancel button also closes the dialog without side effects
- [ ] SpamDialog can be re-opened after being dismissed

## Failure Criteria
- Escape key does not close the dialog
- An API call is fired when Escape is pressed
- Message state changes after dialog dismissal
- Dialog cannot be re-opened after dismissal
- Focus trap prevents Escape from working

## Notes
Tests keyboard accessibility and the non-destructive dismiss paths for SpamDialog. Both Escape and Cancel should have identical behavior: close without action.
