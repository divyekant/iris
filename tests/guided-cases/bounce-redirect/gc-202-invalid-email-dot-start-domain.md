# GC-202: Invalid email — domain starts or ends with dot

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: bounce-redirect
- **Tags**: redirect, validation, email-format, domain
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
1. Send redirect request with domain starting with dot
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@.example.com" }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

2. Send redirect request with domain ending with dot
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@example.com." }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

3. Send redirect request with domain that is only a dot
   - **Target**: `POST /api/messages/{id}/redirect`
   - **Input**: `{ "to": "user@." }`
   - **Expected**: 400 Bad Request with error message indicating invalid email format

## Success Criteria
- [ ] All three requests return 400 status
- [ ] Error messages reference invalid email format
- [ ] No email is sent via SMTP for any attempt

## Failure Criteria
- Any request succeeds (200)
- Server returns 500 instead of 400
- Email is dispatched despite validation failure

## Notes
Validates the "domain not starting/ending with dot" rule. Tests leading dot, trailing dot, and dot-only domain edge cases.
