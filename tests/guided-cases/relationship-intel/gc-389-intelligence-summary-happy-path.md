# GC-389: Happy path — intelligence summary returns top contacts with scores

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, summary, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- Migration 032 (relationship_scores) applied

### Data
- At least one synced account with messages (source: prior sync)
- At least 3 contacts with relationship_scores rows populated (source: scoring pipeline run)

## Steps
1. Fetch intelligence summary for top contacts
   - **Target**: `GET /api/contacts/intelligence/summary`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON array of contact summaries

2. Validate response structure
   - **Target**: Response JSON inspection
   - **Input**: Parse array
   - **Expected**: Array is non-empty; each entry has `email`, `relationship_score`, `total_emails`, `last_contact` fields

3. Validate field values
   - **Target**: Each contact entry
   - **Input**: Inspect numeric and string fields
   - **Expected**: `relationship_score` is a float in [0.0, 100.0], `total_emails` is a positive integer, `last_contact` is an ISO 8601 timestamp string, `email` is a valid email address

4. Verify ordering
   - **Target**: Array order
   - **Input**: Compare `relationship_score` across entries
   - **Expected**: Entries are sorted descending by `relationship_score` (highest score first)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body is a non-empty JSON array
- [ ] Each entry contains `email`, `relationship_score`, `total_emails`, `last_contact`
- [ ] `relationship_score` values are in range [0.0, 100.0]
- [ ] `total_emails` is a positive integer for each entry
- [ ] `last_contact` is a valid ISO 8601 timestamp
- [ ] Array is ordered by `relationship_score` descending

## Failure Criteria
- Non-200 status code
- Empty array when scored contacts exist
- Missing required fields on any entry
- `relationship_score` outside [0.0, 100.0]
- Array is not sorted by score

## Notes
Primary happy path for the summary endpoint. Confirms the relationship_scores table is populated and the summary query aggregates + sorts correctly. The default limit is 10.
