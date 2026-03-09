# GC-067: Block Sender Directly from Settings UI

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: spam-block
- **Tags**: blocked-senders, settings, block, ui, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Browser open to Iris

### Data
- Note the current count of blocked senders (may be zero)

## Steps

1. Navigate to Settings
   - **Target**: Settings page at http://localhost:3000/#/settings
   - **Expected**: Settings page loads with a Blocked Senders section visible

2. Locate the Block Sender form
   - **Target**: Blocked Senders section in Settings
   - **Expected**: An email input field and a "Block Sender" (or similar) submit button are visible; existing blocked senders (if any) are listed in a table

3. Enter a sender email and submit
   - **Target**: Email input field in Block Sender form
   - **Input**: `manual-block-067@example.com`
   - **Expected**: After submitting, the new sender appears in the blocked senders table with the entered email address

4. Verify the new entry in the table
   - **Target**: Blocked Senders table in Settings
   - **Expected**: A new row shows `manual-block-067@example.com` with a timestamp and an Unblock button

5. Verify via API
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response includes an entry with `email_address: "manual-block-067@example.com"`

6. Unblock the sender via the UI
   - **Target**: Unblock button on the newly added row
   - **Expected**: The row is removed from the table; confirmation feedback is shown

7. Verify unblock via API
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: The entry for `manual-block-067@example.com` is no longer present

## Success Criteria
- [ ] Settings page shows Blocked Senders section
- [ ] Email input accepts and submits a new blocked sender
- [ ] New blocked sender appears in the table immediately after submission
- [ ] API confirms the blocked sender was persisted
- [ ] Unblock button removes the sender from the table
- [ ] API confirms the sender was removed after unblocking
- [ ] No console errors during the flow

## Failure Criteria
- Blocked Senders section is missing from Settings
- Form submission fails or does not update the table
- API does not reflect the UI action
- Unblock button does not work or leaves stale entries
- JavaScript errors in browser console

## Notes
Tests the full block/unblock lifecycle entirely from the Settings UI, verifying that UI state and API state remain synchronized. This is distinct from the SpamDialog flow (GC-065) which blocks as a side effect of reporting spam.
