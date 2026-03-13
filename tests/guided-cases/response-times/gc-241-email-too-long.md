# GC-241: Email exceeds 320-char limit rejected with 400

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, validation, length-limit
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- None required

## Steps
1. Construct an email address exceeding 320 characters
   - **Target**: `GET /api/contacts/{email}/response-times`
   - **Input**: email = `aaaa...aaa@example.com` where total length is 321+ characters (local part padded with 'a' characters)
   - **Expected**: 400 Bad Request with error message indicating email too long

## Success Criteria
- [ ] Response status is 400
- [ ] Error message references email length exceeding maximum
- [ ] A 320-char email (exactly at limit) is accepted (not rejected)

## Failure Criteria
- Request succeeds (200) for a 321+ char email
- Server returns 500 instead of 400
- Boundary value (320 chars) is incorrectly rejected

## Notes
RFC 5321 limits email addresses to 320 characters (64 local + @ + 255 domain). This tests the server-side enforcement of that limit. Step success criterion 3 is a boundary check that can be verified in a follow-up request.
