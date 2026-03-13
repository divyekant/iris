# GC-419: Compute Relationships — Happy Path Returns Category Counts

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, compute, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- At least one synced account with messages in the database (source: IMAP sync or seed data)
- At least one contact with both sent and received threads so scoring factors are non-zero

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger relationship score computation
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no request body
   - **Expected**: 200 OK with JSON body containing `computed`, `strong`, `regular`, `weak`, `dormant` fields

3. Validate the computed count
   - **Target**: Response JSON from step 2
   - **Input**: Inspect `computed` field
   - **Expected**: `computed` is a non-negative integer; given at least one contact it should be >= 1

4. Validate category counts sum correctly
   - **Target**: Response JSON from step 2
   - **Input**: Sum `strong + regular + weak + dormant`
   - **Expected**: Sum equals `computed`; all four category fields are non-negative integers

5. Validate strength label boundaries are applied
   - **Target**: Response JSON from step 2
   - **Input**: Inspect distribution across `strong`, `regular`, `weak`, `dormant`
   - **Expected**: At least one category is > 0 when `computed` >= 1; no negative values

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains all five fields: `computed`, `strong`, `regular`, `weak`, `dormant`
- [ ] `computed` is a non-negative integer >= 1 (given at least one contact)
- [ ] `strong + regular + weak + dormant` equals `computed`
- [ ] No field is negative
- [ ] No server error or crash

## Failure Criteria
- Non-200 status code
- Any of the five expected fields is missing from the response
- `strong + regular + weak + dormant` does not equal `computed`
- Any field value is negative
- Server returns 500 or panics

## Notes
Primary happy path for the compute endpoint. Confirms the 5-factor scoring pipeline runs end-to-end (frequency, recency, reciprocity, response_time, thread_engagement), persists results, and returns a correctly bucketed summary. A subsequent GET /api/contacts/relationships should reflect the newly computed scores.
