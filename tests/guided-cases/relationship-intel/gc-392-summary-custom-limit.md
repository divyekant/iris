# GC-392: Summary with custom limit parameter — correct number of results returned

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, summary, limit, pagination
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- Migration 032 (relationship_scores) applied

### Data
- At least 5 contacts with relationship_scores rows (source: scoring pipeline)

## Steps
1. Fetch summary with limit=3
   - **Target**: `GET /api/contacts/intelligence/summary?limit=3`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array of exactly 3 entries

2. Fetch summary with limit=1
   - **Target**: `GET /api/contacts/intelligence/summary?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array of exactly 1 entry — the highest-scored contact

3. Fetch summary with default (no limit param)
   - **Target**: `GET /api/contacts/intelligence/summary`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with up to 10 entries (default limit)

4. Confirm limit=3 subset matches top of default list
   - **Target**: Compare results from steps 1 and 3
   - **Input**: First 3 entries from step 3 response
   - **Expected**: The 3 entries from step 1 match the first 3 entries from step 3 in the same order

## Success Criteria
- [ ] `limit=3` returns exactly 3 entries
- [ ] `limit=1` returns exactly 1 entry (the top contact)
- [ ] Default (no param) returns up to 10 entries
- [ ] The `limit=3` results are the same top-3 as the default list
- [ ] All responses return 200 status

## Failure Criteria
- `limit=3` returns more or fewer than 3 entries (when >= 3 contacts exist)
- `limit=1` does not return the highest-scored contact
- Default list returns more than 10 entries
- Different ordering between `limit=3` and the first 3 of the default list

## Notes
Validates the `limit` query parameter is correctly applied to the summary endpoint. The default documented value is 10. This test also indirectly confirms stable ordering — the same contacts should appear in the same rank regardless of which limit value is used.
