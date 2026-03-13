# GC-203: Redirect nonexistent message — 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, not-found, error-handling
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- A message ID that does not exist in the database (e.g., `99999999` or a UUID that was never created)

## Steps
1. Send redirect request for nonexistent message
   - **Target**: `POST /api/messages/99999999/redirect`
   - **Input**: `{ "to": "valid@example.com" }`
   - **Expected**: 404 Not Found with error message indicating message does not exist

## Success Criteria
- [ ] Response status is 404
- [ ] Error message indicates the message was not found
- [ ] No email is sent via SMTP

## Failure Criteria
- Request succeeds (200) or returns 500
- Server panics or returns an unstructured error
- Any SMTP send is attempted

## Notes
Verifies the server checks message existence before attempting redirect. The 404 should be returned before any SMTP or OAuth operations occur.
