# GC-207: UI success confirmation flow via RedirectDialog

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, ui, success-state, dialog
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- Frontend accessible at http://localhost:5173 (or built SPA at :3000)

### Data
- Existing message ID with a valid `from_address` and subject line (source: seed or prior sync)
- Active account linked to the message

## Steps
1. Open RedirectDialog for a message
   - **Target**: UI — click redirect action on a message in ThreadView
   - **Input**: Navigate to a thread, trigger redirect action
   - **Expected**: RedirectDialog modal opens showing original sender and subject

2. Verify modal displays original message metadata
   - **Target**: RedirectDialog.svelte
   - **Input**: Inspect displayed sender and subject
   - **Expected**: Original sender address and subject line are visible in the dialog

3. Enter valid recipient and submit
   - **Target**: RedirectDialog email input
   - **Input**: Type `recipient@example.com`, press Cmd+Enter
   - **Expected**: Loading spinner appears, then success confirmation is displayed

4. Verify success state
   - **Target**: RedirectDialog post-submit state
   - **Input**: Observe the dialog after API returns 200
   - **Expected**: Success message shown (e.g., "Redirected to recipient@example.com"), dialog can be dismissed

5. Dismiss dialog with Escape
   - **Target**: RedirectDialog
   - **Input**: Press Escape key
   - **Expected**: Dialog closes, returns to ThreadView

## Success Criteria
- [ ] Dialog opens with correct sender and subject from the original message
- [ ] Cmd+Enter triggers the redirect API call
- [ ] Loading spinner is visible during the API call
- [ ] Success confirmation is displayed after successful redirect
- [ ] Escape key closes the dialog
- [ ] Underlying ThreadView is not affected after dialog dismissal

## Failure Criteria
- Dialog opens with missing or incorrect message metadata
- Cmd+Enter does not trigger submission
- No loading indicator during API call
- No success feedback after redirect completes
- Escape key does not close the dialog

## Notes
This case validates the full UI flow through RedirectDialog.svelte. The API assertions from GC-199 apply to the underlying request; this case focuses on the user-facing interaction states.
