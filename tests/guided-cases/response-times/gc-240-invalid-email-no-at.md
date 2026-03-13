# GC-240: Invalid email — missing @ symbol rejected with 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, validation, email-format
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Request response times with an email missing @
   - **Target**: `GET /api/contacts/aliceexample.com/response-times`
   - **Input**: email path param = `aliceexample.com` (no @ sign)
   - **Expected**: 400 Bad Request with error message indicating invalid email

## Success Criteria
- [ ] Response status is 400
- [ ] Error message references invalid email format
- [ ] No database query is executed for the invalid input

## Failure Criteria
- Request succeeds (200) with empty/zero stats instead of rejecting
- Server returns 500 instead of 400
- Error message is generic or unhelpful

## Notes
Validates the "email must contain @" check. The API should reject at the validation layer before touching the database.
