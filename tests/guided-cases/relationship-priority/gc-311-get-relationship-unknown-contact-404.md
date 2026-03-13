# GC-311: Get Relationship for Unknown Contact Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, 404, unknown-contact, not-found
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- `nobody@example.com` does not exist in the `relationship_scores` table (source: verified by absence — use an address never seen in any synced messages)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request relationship score for an address with no history
   - **Target**: `GET http://localhost:3000/api/contacts/nobody@example.com/relationship`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. Verify error response does not leak internal details
   - **Target**: Response body from step 2
   - **Input**: Inspect body for SQL fragments, stack traces, or internal paths
   - **Expected**: Body is either empty, a generic `{"error": "..."}` message, or a human-readable string with no internal implementation details

## Success Criteria
- [ ] Response status is 404
- [ ] No 200 with empty/zero scores returned
- [ ] No server error (500) returned
- [ ] Response body does not contain SQL, stack traces, or file paths

## Failure Criteria
- Status is 200 (zero-score fabrication is incorrect; 404 is the contract)
- Status is 500
- Response reveals internal database structure or Rust panic messages

## Notes
The API contract specifies 404 when no score exists for the given email. Callers must distinguish "no relationship data" from "zero score" — returning a zero-score object would mask missing data. Run POST /api/ai/relationship-priority first if the DB is empty, then confirm this address was still not scored.
