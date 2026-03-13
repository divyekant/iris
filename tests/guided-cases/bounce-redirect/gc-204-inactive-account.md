# GC-204: Redirect from inactive account — 400

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, inactive-account, error-handling
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- An account marked as inactive in the database (source: manually set `is_active = 0` or use a deactivated test account)
- A message belonging to that inactive account

## Steps
1. Send redirect request from message on inactive account
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "recipient@example.com" }`
   - **Expected**: 400 Bad Request with error message indicating the account is inactive

## Success Criteria
- [ ] Response status is 400
- [ ] Error message references inactive or disabled account
- [ ] No email is sent via SMTP
- [ ] No OAuth token refresh is attempted

## Failure Criteria
- Request succeeds (200) and email is sent from an inactive account
- Server returns 404 or 500 instead of 400
- OAuth token refresh is triggered for the inactive account

## Notes
Inactive accounts should be caught early in the redirect flow. The server must not attempt OAuth refresh or SMTP send for deactivated accounts.
