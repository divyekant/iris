# GC-045: Cancel Already-Sent Message Fails Gracefully

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: cancel, already-sent, negative, error-handling, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- A pending send that has already been processed (status = "sent") (source: create a send with minimal undo delay, wait for it to be processed, or directly manipulate a known sent record)

## Steps

1. Create a send with minimal delay to ensure it gets processed
   - **Target**: `PUT /api/config/undo-send-delay` then `POST /api/send`
   - **Input**: Set undo delay to 5 seconds, then send a valid email without `schedule_at`
   - **Expected**: Send created with `send_at` approximately 5 seconds in the future, returns send `id`

2. Wait for the send to be processed
   - **Target**: Wait 10 seconds (or poll until status changes)
   - **Expected**: The background worker processes the send, status transitions to "sent"

3. Attempt to cancel the already-sent message via undo endpoint
   - **Target**: `POST /api/send/cancel/{id}` (using `id` from step 1)
   - **Expected**: Error response (4xx) indicating the send has already been processed and cannot be cancelled

4. Attempt to cancel via the scheduled delete endpoint
   - **Target**: `DELETE /api/send/scheduled/{id}` (using `id` from step 1)
   - **Expected**: Error response (4xx or 404) indicating the item is not a pending scheduled send

## Success Criteria
- [ ] `POST /api/send/cancel/{id}` for an already-sent message returns an error (not 200)
- [ ] Error response includes a meaningful message (e.g., "already sent", "cannot cancel")
- [ ] `DELETE /api/send/scheduled/{id}` for an already-sent message returns an error
- [ ] No data corruption — the sent record remains with `status: "sent"`
- [ ] No duplicate send triggered by the cancel attempt

## Failure Criteria
- Cancel endpoint returns 200 for an already-sent message
- The sent message's status is changed back to "cancelled" after it was already sent
- Server returns 500 or crashes
- Cancel attempt causes the message to be sent again

## Notes
This tests the race condition safety of the cancel flow. The API should check `status = 'pending'` in the WHERE clause of the UPDATE, so cancelling an already-sent message is a no-op that returns an appropriate error.
