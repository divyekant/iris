# GC-060: List Blocked Senders After Blocking

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: spam-block
- **Tags**: blocked-senders, list, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No blocked senders exist initially (or note the current count for comparison)

## Steps

1. Check baseline blocked senders list
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with an array (may be empty); note the count

2. Block a sender directly
   - **Target**: `POST /api/blocked-senders`
   - **Input**: `{"email_address": "spammer-060@example.com", "reason": "test block for GC-060"}`
   - **Expected**: Response 200 with a BlockedSender object containing `id`, `email_address`, `reason`, and `created_at`

3. Block a second sender
   - **Target**: `POST /api/blocked-senders`
   - **Input**: `{"email_address": "spammer-060b@example.com", "reason": "second test block"}`
   - **Expected**: Response 200 with a BlockedSender object for the second sender

4. List all blocked senders
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with an array containing both newly blocked senders, each with correct `email_address`, `reason`, and valid `created_at` timestamps

## Success Criteria
- [ ] GET /api/blocked-senders returns 200
- [ ] Response is an array of BlockedSender objects
- [ ] Each object has `id`, `email_address`, `reason`, and `created_at` fields
- [ ] Both blocked senders from steps 2 and 3 appear in the list
- [ ] Count increased by 2 relative to baseline

## Failure Criteria
- GET /api/blocked-senders returns non-200 status
- Response is not an array or is missing expected fields
- One or both blocked senders are missing from the list
- `created_at` timestamps are null or malformed

## Notes
Validates the full create-then-list cycle for blocked senders. Uses the direct POST /api/blocked-senders endpoint rather than the report-spam flow.
