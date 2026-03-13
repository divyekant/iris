# GC-423: List Relationships Filtered by Strength

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, filter, list
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed with at least one contact in the `strong` bucket (overall_score >= 0.7) (source: POST /api/contacts/relationships/compute)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores and confirm at least one strong contact
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `strong` >= 1

3. List relationships filtered to strong contacts only
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?strength=strong&sort=score&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a JSON array containing only contacts whose `strength_label` is `"strong"`

4. Validate every returned entry meets the strong threshold
   - **Target**: Each entry in the array from step 3
   - **Input**: Inspect `overall_score` and `strength_label` on each entry
   - **Expected**: Every entry has `strength_label == "strong"` and `overall_score >= 0.7`

5. List relationships filtered to weak contacts only
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?strength=weak&sort=score&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with entries where all `strength_label == "weak"` and `0.15 <= overall_score < 0.4`

6. Verify filtered results are a strict subset of unfiltered results
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score&limit=20&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; every email from the strong-filtered response (step 3) also appears in this unfiltered response

## Success Criteria
- [ ] Response status is 200 on all requests
- [ ] Step 3 returns only entries with `strength_label == "strong"`
- [ ] All entries in step 3 have `overall_score >= 0.7`
- [ ] Step 5 returns only entries with `strength_label == "weak"` and score in [0.15, 0.4)
- [ ] Filtered results are a subset of the unfiltered list
- [ ] Empty array (not an error) when no contacts match the requested strength

## Failure Criteria
- Non-200 status code on any request
- Any entry in the strength-filtered response has a `strength_label` that does not match the filter
- Any `overall_score` in the `strong` response is below 0.7
- Server returns 500 for a valid strength filter value

## Notes
Tests the `strength` query parameter as a strict filter. Valid values are `strong`, `regular`, `weak`, `dormant`. If no contacts exist in a requested strength bucket the expected response is an empty array with status 200, not a 404. Also tests `regular` and `dormant` implicitly via the unfiltered baseline comparison.
