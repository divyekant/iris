# GC-231: Contact Topics Invalid Email — Missing @ Symbol

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, validation, email
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No specific data required

## Steps
1. Request topics with an email missing the @ symbol
   - **Target**: `GET /api/contacts/not-an-email/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request

2. Verify error response
   - **Target**: Response body
   - **Input**: Parse JSON or text
   - **Expected**: Error message indicates invalid email format

3. Try another invalid email variant
   - **Target**: `GET /api/contacts/justadomain.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Response status is 400 for `not-an-email`
- [ ] Response status is 400 for `justadomain.com`
- [ ] Error message references invalid email format
- [ ] No AI call is triggered
- [ ] No database query is executed for the invalid email

## Failure Criteria
- Response status is 200 (validation bypassed)
- Response status is 500 (unhandled error)
- Server processes the request and calls AI with invalid email

## Notes
The validation rule is simple: email must contain '@'. This is a minimal check — it does not validate full RFC 5322 compliance. The goal is to reject obviously invalid input before hitting the database or AI layer.
