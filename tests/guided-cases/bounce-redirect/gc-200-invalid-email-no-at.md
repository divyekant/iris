# GC-200: Invalid email — missing @ symbol

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, validation, email-format
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Existing message ID with a valid `from_address` (source: seed or prior sync)
- Active account linked to the message

## Steps
1. Send redirect request with email missing @
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "colleagueexample.com" }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

## Success Criteria
- [ ] Response status is 400
- [ ] Error message references invalid email format
- [ ] No email is sent via SMTP

## Failure Criteria
- Request succeeds (200) with an invalid email
- Server returns 500 instead of 400
- Email is dispatched despite validation failure

## Notes
Validates the "exactly one @" requirement. An address with zero @ signs must be rejected at the API layer before any SMTP interaction.
