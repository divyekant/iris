# GC-243: No account emails found — returns zeros and nulls

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, edge-case, no-accounts, zeros
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- No active email accounts configured (all accounts deleted or none added), OR test against a fresh database with no accounts

## Steps
1. Request response times when no accounts exist
   - **Target**: `GET /api/contacts/someone@example.com/response-times`
   - **Input**: email = `someone@example.com`
   - **Expected**: 200 OK with zeroed/null stats (same shape as no-shared-threads)

2. Verify response shape
   - **Target**: Response JSON inspection
   - **Input**: Check all fields
   - **Expected**: `their_reply_count` = 0, `your_reply_count` = 0, `total_exchanges` = 0, all hour fields null

## Success Criteria
- [ ] Response status is 200
- [ ] All count fields are 0
- [ ] All hour fields are null
- [ ] No server error or crash when accounts table is empty

## Failure Criteria
- Server returns 500 due to empty accounts
- Response is 404 or 403
- Any non-zero/non-null values returned

## Notes
Tests graceful degradation when the system has no account emails to match against. The algorithm should short-circuit and return the zero/null response without attempting thread analysis.
