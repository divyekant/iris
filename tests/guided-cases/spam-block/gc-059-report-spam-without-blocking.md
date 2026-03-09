# GC-059: Report Spam Without Blocking Sender

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: spam-block
- **Tags**: spam, report-spam, no-block, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- At least one message exists in the inbox (source: IMAP sync or prior test setup)
- Note the message ID and sender email of a target message

## Steps

1. Identify a target message
   - **Target**: `GET /api/messages?folder=inbox&limit=1`
   - **Expected**: Response 200 with at least one message object

2. Report the message as spam without blocking
   - **Target**: `POST /api/messages/report-spam`
   - **Input**: `{"ids": ["<message_id>"], "block_sender": false}`
   - **Expected**: Response 200 with body `{"updated": 1, "blocked_sender": null}`

3. Verify the message moved to spam folder
   - **Target**: `GET /api/messages?folder=spam`
   - **Expected**: The reported message appears in the spam folder results

4. Verify the sender is NOT blocked
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with an array that does NOT contain the sender's email address

## Success Criteria
- [ ] POST /api/messages/report-spam returns 200
- [ ] Response body `updated` field equals 1
- [ ] Response body `blocked_sender` field is null
- [ ] Message moves from inbox to spam folder
- [ ] Sender does NOT appear in blocked-senders list

## Failure Criteria
- report-spam returns non-200 status
- `blocked_sender` is non-null when block_sender was false
- Message remains in inbox after reporting
- Sender incorrectly appears in blocked-senders list

## Notes
Tests that the spam report flow works independently of the block-sender feature. When `block_sender` is false (or omitted), only the folder move should occur.
