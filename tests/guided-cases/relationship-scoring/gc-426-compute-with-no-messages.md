# GC-426: Compute Relationships with No Messages in Database

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, compute, empty-db, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Fresh Iris instance with no synced accounts or an account that has been synced but has zero messages in the `messages` table (source: fresh Docker container or cleared test database)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Verify the database is in an empty message state
   - **Target**: `GET http://127.0.0.1:3000/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with an empty array or `total: 0` — confirms no messages are present before the compute call

3. Trigger relationship score computation on the empty database
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no request body
   - **Expected**: 200 OK — the server handles zero messages gracefully without error

4. Validate the response reflects zero computed contacts
   - **Target**: Response JSON from step 3
   - **Input**: Inspect `computed`, `strong`, `regular`, `weak`, `dormant`
   - **Expected**: `computed` is 0; all category counts are 0; no 500 or panic

5. Verify stats endpoint also returns zeros gracefully
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with all counts at 0; `most_active` field is null or absent

6. Verify list endpoint returns empty array when no scores exist
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with an empty JSON array — not a 404 or 500

## Success Criteria
- [ ] Step 3 returns 200 (not 500 or 400)
- [ ] `computed` is 0 in step 3 response
- [ ] All category counts (`strong`, `regular`, `weak`, `dormant`) are 0
- [ ] Stats endpoint (step 5) returns 200 with zero counts
- [ ] List endpoint (step 6) returns 200 with empty array
- [ ] Server remains healthy throughout — no panics

## Failure Criteria
- Step 3 returns 500 or panics on an empty messages table
- `computed` is non-zero when no messages exist
- Any category count is negative
- Stats or list endpoint returns 500 after a zero-compute run
- Server crashes and subsequent requests fail

## Notes
Empty-state handling is a critical robustness requirement. The scoring algorithm iterates over contacts derived from message history; when no messages exist there are no contacts to score. The endpoint must short-circuit cleanly and return a valid zero-state response rather than propagating a divide-by-zero or null-dereference from the scoring math. This case is most easily reproduced using a fresh Docker container or by clearing the `messages` and `contacts` tables before the test.
