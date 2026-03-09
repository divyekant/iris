# GC-061: Unblock a Sender via API

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: spam-block
- **Tags**: blocked-senders, unblock, delete, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- At least one blocked sender exists (create one via `POST /api/blocked-senders` if needed)

## Steps

1. Block a sender to ensure a target exists
   - **Target**: `POST /api/blocked-senders`
   - **Input**: `{"email_address": "unblock-test-061@example.com", "reason": "will be unblocked"}`
   - **Expected**: Response 200 with a BlockedSender object; note the `id` field

2. Verify the sender appears in the blocked list
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with the sender from step 1 present in the array

3. Unblock the sender
   - **Target**: `DELETE /api/blocked-senders/<id_from_step_1>`
   - **Expected**: Response 204 No Content

4. Verify the sender is removed from the blocked list
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with the sender from step 1 no longer present in the array

## Success Criteria
- [ ] POST /api/blocked-senders returns 200 with valid BlockedSender
- [ ] DELETE /api/blocked-senders/{id} returns 204
- [ ] Sender no longer appears in GET /api/blocked-senders after deletion
- [ ] Other blocked senders (if any) remain unaffected

## Failure Criteria
- DELETE returns non-204 status for a valid ID
- Sender still appears in blocked list after DELETE
- Other blocked senders are inadvertently removed

## Notes
Tests the complete block-then-unblock lifecycle. The DELETE endpoint should be idempotent in effect: after deletion, the sender is simply gone from the list.
