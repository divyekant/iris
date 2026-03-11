# GC-184: Unsubscribe on Nonexistent Message Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, 404, nonexistent
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. POST to the unsubscribe endpoint with a message ID that does not exist in the database
   - **Target**: POST /api/messages/nonexistent-message-id-00000/unsubscribe
   - **Input**: none
   - **Expected**: 404 Not Found

## Success Criteria
- [ ] Response status is 404
- [ ] Response body (if any) indicates message was not found
- [ ] No server-side unsubscribe action is triggered

## Failure Criteria
- Response status is 200 or 500
- Server panics or returns an unhandled error
