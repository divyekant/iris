# GC-183: Unsubscribe on Message Without Header Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, 404, no-header
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists that has NO `List-Unsubscribe` header (i.e., `list_unsubscribe` is null); note its message ID

## Steps
1. POST to the unsubscribe endpoint for the message with no unsubscribe URL
   - **Target**: POST /api/messages/{id}/unsubscribe
   - **Input**: none
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Response status is 404
- [ ] No side effects occur (no emails sent, no external requests made)

## Failure Criteria
- Response status is 200 (unsubscribe attempted with no URL)
- Response status is 500 (server error instead of clean 404)
- Any external request is triggered
