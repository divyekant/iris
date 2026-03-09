# GC-058: Report Spam and Block Sender via API

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: spam-block
- **Tags**: spam, block-sender, report-spam, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- At least one message exists in the inbox (source: IMAP sync or prior test setup)
- Note the message ID of a message whose sender you want to block

## Steps

1. Identify a target message
   - **Target**: `GET /api/messages?folder=inbox&limit=1`
   - **Expected**: Response 200 with at least one message object containing an `id` field

2. Report the message as spam with block_sender enabled
   - **Target**: `POST /api/messages/report-spam`
   - **Input**: `{"ids": ["<message_id>"], "block_sender": true}`
   - **Expected**: Response 200 with body `{"updated": 1, "blocked_sender": "<sender_email>"}`

3. Verify the message moved to spam folder
   - **Target**: `GET /api/messages?folder=spam`
   - **Expected**: The reported message appears in the spam folder results

4. Verify the sender is now blocked
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with an array containing an entry where `email_address` matches the sender from step 2

## Success Criteria
- [ ] POST /api/messages/report-spam returns 200
- [ ] Response body `updated` field equals 1
- [ ] Response body `blocked_sender` field contains the sender's email address
- [ ] Message no longer appears in inbox
- [ ] Message appears in spam folder
- [ ] GET /api/blocked-senders includes the sender's email

## Failure Criteria
- report-spam returns non-200 status
- `updated` is 0 or `blocked_sender` is null when block_sender was true
- Message remains in inbox after reporting
- Sender does not appear in blocked-senders list

## Notes
This is the primary happy-path test for the combined spam+block flow. The `block_sender` flag triggers a lookup of the `from_address` from the first message ID in the array.
