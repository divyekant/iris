# GC-064: Unblock Non-Existent Sender Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: spam-block
- **Tags**: blocked-senders, unblock, not-found, negative, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No blocked sender with ID 999999 exists (or use any ID confirmed to not exist)

## Steps

1. Attempt to unblock a sender with a non-existent ID
   - **Target**: `DELETE /api/blocked-senders/999999`
   - **Expected**: Response 404 Not Found

2. Attempt to unblock with an obviously invalid ID format
   - **Target**: `DELETE /api/blocked-senders/not-a-number`
   - **Expected**: Response 400 Bad Request or 404 Not Found (depending on route parsing)

3. Verify the blocked-senders list is unchanged
   - **Target**: `GET /api/blocked-senders`
   - **Expected**: Response 200 with the same list as before the DELETE attempts

## Success Criteria
- [ ] DELETE with non-existent numeric ID returns 404
- [ ] DELETE with invalid ID format returns 400 or 404
- [ ] No side effects on the blocked-senders list
- [ ] Response bodies contain appropriate error context

## Failure Criteria
- Server returns 200 or 204 for a non-existent ID
- Server returns 500 (unhandled error)
- Blocked senders list is modified by the failed DELETE

## Notes
Ensures the DELETE endpoint handles missing resources gracefully. The invalid-format test (step 2) verifies route-level validation or type coercion behavior.
