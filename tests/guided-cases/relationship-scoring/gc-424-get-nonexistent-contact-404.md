# GC-424: Get Relationship for Non-Existent Contact Returns 404 or Zero Scores

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, not-found, 404, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- `ghost@nonexistent-domain-iris-test.com` does not exist in the relationships table (source: verified by absence — this address should never appear in any synced mailbox)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request relationship detail for an address with no history
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/ghost@nonexistent-domain-iris-test.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found — no relationship row exists for this address

3. Verify the error response does not leak internal details
   - **Target**: Response body from step 2
   - **Input**: Inspect body for SQL fragments, stack traces, column names, or Rust panic messages
   - **Expected**: Body is either empty, a generic `{"error": "..."}` object, or a human-readable string with no internal implementation details

4. Confirm server remains healthy after the 404
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK — server is fully operational; the 404 was a clean not-found, not a crash

## Success Criteria
- [ ] Response status is 404 for the unknown contact
- [ ] No 200 with zero-score fabrication (the contact has no data — returning synthetic zeros would misrepresent the contract)
- [ ] Response body does not contain SQL, stack traces, column names, or file paths
- [ ] Subsequent request to stats endpoint returns 200 (server is stable)

## Failure Criteria
- Response status is 200 (zero-score fabrication)
- Response status is 500
- Response body reveals internal database structure, Rust panic messages, or SQL fragments
- Server becomes unresponsive after the 404

## Notes
The API contract specifies 404 when no relationship data exists for the given email. Callers must distinguish "no relationship computed" from "dormant relationship" — returning a zero-score object would mask missing data and could mislead callers into treating a never-seen contact as a scored dormant one. If the implementation returns a zero-score object instead of 404, record the discrepancy as a contract violation bug.
