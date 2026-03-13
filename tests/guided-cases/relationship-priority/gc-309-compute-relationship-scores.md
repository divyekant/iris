# GC-309: Compute Relationship Scores for All Contacts

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, compute, happy-path, scored-count
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account with messages (source: prior sync)
- At least one contact with sent/received threads in the database (source: seed or real inbox)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger relationship score computation for all contacts
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: Header `X-Session-Token: {token}`, no request body
   - **Expected**: 200 OK with JSON body `{ "scored": <N> }` where N >= 1

3. Verify the scored count reflects actual contacts
   - **Target**: Response JSON from step 2
   - **Input**: Inspect `scored` field
   - **Expected**: `scored` is a non-negative integer; with at least one contact in the DB it should be >= 1

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains a `scored` field
- [ ] `scored` is an integer >= 1 (given at least one contact exists)
- [ ] No server error or crash

## Failure Criteria
- Non-200 status code
- Missing `scored` field in response
- Server returns 500 or panics
- `scored` is negative

## Notes
Primary happy path for the compute endpoint. Confirms the scoring pipeline runs end-to-end and persists results to the `relationship_scores` table. A subsequent GET /api/contacts/{email}/relationship call should return data for any scored contact.
