# GC-201: Invalid email — double @ symbol

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
1. Send redirect request with email containing two @ symbols
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@@example.com" }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

2. Send redirect request with @ in both local and domain parts
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@name@example.com" }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

## Success Criteria
- [ ] Both requests return 400 status
- [ ] Error messages reference invalid email format
- [ ] No email is sent via SMTP for either attempt

## Failure Criteria
- Either request succeeds (200)
- Server returns 500 instead of 400
- Email is dispatched despite validation failure

## Notes
Validates the "exactly one @" rule. Multiple @ signs in any position must be rejected. Tests two variants: adjacent @@ and separated @...@.
