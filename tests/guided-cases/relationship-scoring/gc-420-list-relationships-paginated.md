# GC-420: List Relationships — Paginated Response

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, list, pagination, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed (source: POST /api/contacts/relationships/compute)
- At least 3 contacts with computed scores so pagination is meaningful

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `computed` >= 3

3. Fetch the first page with a small limit
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score&limit=2&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array containing exactly 2 entries (or fewer if fewer than 2 contacts exist)

4. Validate response entry structure
   - **Target**: Each entry in the response array from step 3
   - **Input**: Inspect fields on each item
   - **Expected**: Each entry contains `email`, `overall_score`, `strength_label`, and at minimum the five factor scores (`frequency`, `recency`, `reciprocity`, `response_time`, `thread_engagement`)

5. Verify sort order by score descending
   - **Target**: Response array from step 3
   - **Input**: Compare `overall_score` across entries
   - **Expected**: Entries are ordered descending by `overall_score` (highest first)

6. Fetch the second page
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships?sort=score&limit=2&offset=2`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; if fewer than 3 contacts exist the array may be empty — that is acceptable; no entry should duplicate entries from step 3

7. Verify no duplicates across pages
   - **Target**: Email values from step 3 and step 6 responses
   - **Input**: Collect all `email` values from both pages
   - **Expected**: No email appears in both pages

## Success Criteria
- [ ] Response status is 200 on all requests
- [ ] Step 3 returns at most 2 entries (respects `limit=2`)
- [ ] Each entry has `email`, `overall_score`, `strength_label`, and factor score fields
- [ ] `overall_score` values are in range [0.0, 1.0]
- [ ] Entries in step 3 are ordered by `overall_score` descending
- [ ] No email address appears in both pages

## Failure Criteria
- Non-200 status code on any request
- Response returns more entries than the specified `limit`
- Any entry is missing required fields
- `overall_score` is outside [0.0, 1.0]
- Same email appears on both pages (indicates offset is not applied)
- Server returns 500

## Notes
Validates the core list endpoint with sorting and pagination. The `sort=score` parameter orders by `overall_score` descending. Confirm the response is a JSON array (not a wrapped object) unless the API wraps it — adjust assertions accordingly based on actual response shape.
