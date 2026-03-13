# GC-469: Happy Path — Create Follow-up Tracker

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, create, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Create follow-up tracker for a Sent message
   - **Target**: `POST /api/followup-tracking`
   - **Input**: `{"message_id": "<sent-msg-uuid>", "days": 7, "note": "Follow up on HTML test"}`
   - **Expected**: 200 with tracker object

## Notes
- message_id must be a string UUID of a Sent message (folder="Sent")
- days field is required (1-90 range)
- due_date is not a field; follow-up is computed as sent_at + days*86400

## Result
- **Status**: passed
- **Response**: 200 with full tracker (id, message_id, account_id, thread_id, to_address, subject, status="active", days_remaining, is_overdue)
