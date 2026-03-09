# GC-043: Undo Normal Send (Non-Scheduled)

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: scheduled-send
- **Tags**: undo, send, toast, cancel, non-scheduled
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured
- Undo send delay configured (default or set to a known value, e.g., 10 seconds via `PUT /api/config/undo-send-delay`)

### Data
- A valid recipient email address (source: manual input)
- Known undo delay value (source: `GET /api/config/undo-send-delay`)

## Steps

1. Check current undo send delay
   - **Target**: `GET /api/config/undo-send-delay`
   - **Expected**: Returns `{ delay_seconds: N }` where N is between 5 and 30

2. Open the compose modal and fill in email fields
   - **Target**: Inbox page > Compose button > ComposeModal form fields
   - **Input**: To: valid recipient, Subject: "GC-043 Undo Test", Body: "This send should be undone"
   - **Expected**: ComposeModal open with fields populated

3. Click Send (normal send, no schedule)
   - **Target**: ComposeModal > Send button
   - **Expected**: ComposeModal closes, an undo-send toast appears with a countdown timer showing remaining seconds

4. Click "Undo" on the toast before the countdown expires
   - **Target**: Undo send toast > Undo button
   - **Expected**: Toast dismisses, a confirmation appears that the send was cancelled

5. Verify the send was cancelled via API
   - **Target**: `POST /api/send/cancel/{id}` should have been triggered, or verify via `GET /api/send/scheduled`
   - **Expected**: The pending send entry has `status: "cancelled"`, the email was NOT actually sent

## Success Criteria
- [ ] Normal send shows an undo toast with countdown timer
- [ ] Countdown duration matches the configured undo delay
- [ ] Clicking "Undo" before expiry cancels the send
- [ ] The pending send record transitions to `status: "cancelled"`
- [ ] The email is NOT delivered to the recipient's mail server
- [ ] User receives visual confirmation that the undo succeeded

## Failure Criteria
- No undo toast appears after sending
- Undo button does not cancel the send (email is sent anyway)
- Toast countdown does not match configured delay
- Pending send remains in "pending" status after clicking undo

## Notes
The undo-send mechanism works by inserting into `pending_sends` with `send_at = now + undo_delay`. The background worker picks up sends whose `send_at` has passed. Clicking undo calls `POST /api/send/cancel/{id}` which sets `status='cancelled'` before the worker processes it. Timing is critical — the undo must happen before the worker sends the email.
