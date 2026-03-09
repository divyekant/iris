# GC-062: Block Sender with Empty Email Rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: spam-block
- **Tags**: blocked-senders, validation, negative, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- None required

## Steps

1. Attempt to block a sender with an empty email address
   - **Target**: `POST /api/blocked-senders`
   - **Input**: `{"email_address": "", "reason": "should fail"}`
   - **Expected**: Response 400 Bad Request with an error message indicating email_address must not be empty

2. Attempt to block a sender with a missing email_address field
   - **Target**: `POST /api/blocked-senders`
   - **Input**: `{"reason": "should also fail"}`
   - **Expected**: Response 400 Bad Request (or 422 Unprocessable Entity) with an error message

3. Verify no phantom entries were created
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200; no entries with empty or missing email addresses exist

## Success Criteria
- [ ] Empty email_address returns 400 (or 422) status
- [ ] Missing email_address field returns 400 (or 422) status
- [ ] Response bodies contain meaningful error messages
- [ ] No blocked-sender entries are created for invalid inputs

## Failure Criteria
- Server returns 200 or 201 for empty/missing email_address
- A blocked-sender entry with empty email is persisted
- Server returns 500 (unhandled error) instead of a validation error

## Notes
Input validation is critical for the blocked-senders endpoint. Empty strings and missing fields should both be caught before any database write occurs.
