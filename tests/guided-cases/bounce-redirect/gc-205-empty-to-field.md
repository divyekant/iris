# GC-205: Redirect with empty to field — 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, validation, empty-input
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
1. Send redirect request with empty string
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "" }`
   - **Expected**: 400 Bad Request with error message indicating email address is required

2. Send redirect request with whitespace-only string
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "   " }`
   - **Expected**: 400 Bad Request with error message indicating email address is required or invalid format

3. Send redirect request with missing to field
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{}`
   - **Expected**: 400 Bad Request (deserialization error or missing field error)

## Success Criteria
- [ ] All three requests return 400 status
- [ ] Error messages indicate missing or invalid email
- [ ] No email is sent via SMTP for any attempt

## Failure Criteria
- Any request succeeds (200)
- Server returns 500 or panics on empty/missing input
- Email is dispatched with empty recipient

## Notes
Tests the bottom of the input spectrum. Empty strings, whitespace, and missing fields must all be caught by validation before reaching SMTP logic.
