# GC-063: Report Spam with Empty IDs Rejected

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: spam-block
- **Tags**: spam, report-spam, validation, negative, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- None required

## Steps

1. Attempt to report spam with an empty ids array
   - **Target**: `POST /api/messages/report-spam`
   - **Input**: `{"ids": [], "block_sender": false}`
   - **Expected**: Response 400 Bad Request with an error message indicating ids must be non-empty

2. Attempt to report spam with missing ids field
   - **Target**: `POST /api/messages/report-spam`
   - **Input**: `{"block_sender": true}`
   - **Expected**: Response 400 Bad Request (or 422 Unprocessable Entity) with an error message

3. Attempt to report spam with ids exceeding the 1000-item limit
   - **Target**: `POST /api/messages/report-spam`
   - **Input**: `{"ids": ["id_1", "id_2", ..., "id_1001"], "block_sender": false}` (array of 1001 placeholder IDs)
   - **Expected**: Response 400 Bad Request with an error message indicating ids must be <= 1000

4. Verify no messages were moved
   - **Target**: `GET /api/messages?folder=spam`
   - **Expected**: No new messages in spam folder from these failed requests

## Success Criteria
- [ ] Empty ids array returns 400 status
- [ ] Missing ids field returns 400 (or 422) status
- [ ] Exceeding 1000 ids returns 400 status
- [ ] All error responses contain meaningful error messages
- [ ] No side effects (no messages moved, no senders blocked)

## Failure Criteria
- Server returns 200 for any of the invalid inputs
- Messages are moved despite validation failure
- A sender is blocked despite validation failure
- Server returns 500 (unhandled error) instead of a validation error

## Notes
Tests all three validation boundaries for the `ids` field: empty array, missing field, and exceeding the 1000-item cap. The over-limit test requires constructing an array of 1001 strings.
