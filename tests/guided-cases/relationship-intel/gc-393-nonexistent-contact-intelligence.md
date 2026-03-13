# GC-393: Negative — intelligence detail for non-existent contact returns empty stats or 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, not-found, empty-state
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- An email address that has never appeared in any synced message (e.g., `nobody@does-not-exist.test`)

## Steps
1. Request intelligence for a non-existent contact
   - **Target**: `GET /api/contacts/nobody@does-not-exist.test/intelligence`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Either 404 Not Found OR 200 OK with all-zero/null stats

2. If 200 received — validate empty stats shape
   - **Target**: Response JSON inspection
   - **Input**: Parse JSON fields
   - **Expected**: `relationship_score` = 0 or null, `stats.total_emails` = 0, `stats.sent_by_you` = 0, `stats.received` = 0, `stats.avg_response_time` = null, `common_topics` = [], `communication_patterns.avg_emails_per_week` = 0

3. If 404 received — validate error body
   - **Target**: Response JSON inspection
   - **Input**: Parse JSON
   - **Expected**: Body contains an error message string; no server stack trace or internal details exposed

## Success Criteria
- [ ] Response is either 404 or 200 (no 500)
- [ ] If 200: all stat fields are zero or null, topics array is empty
- [ ] If 404: error message is present, no internal details exposed
- [ ] No non-zero stats returned for an unknown contact

## Failure Criteria
- 500 Internal Server Error
- 200 with non-zero `total_emails` for an address with no messages
- 200 with non-null `avg_response_time` for an address with no messages
- Stack trace or SQL error exposed in response body

## Notes
The API may return either 200 with empty data or 404 — either is acceptable. The important constraint is that zero emails means zero stats. This test confirms no data leakage from adjacent contacts and no server crash on an unknown address.
