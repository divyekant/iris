# GC-242: No shared threads — returns zeros and nulls

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, edge-case, no-data, zeros
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account (source: prior sync)
- A valid email address that has no messages in any shared thread (e.g., `unknown-contact@nowhere.test`)

## Steps
1. Request response times for a contact with no shared threads
   - **Target**: `GET /api/contacts/unknown-contact@nowhere.test/response-times`
   - **Input**: email = `unknown-contact@nowhere.test`
   - **Expected**: 200 OK with zeroed/null stats

2. Verify response shape
   - **Target**: Response JSON inspection
   - **Input**: Check all fields
   - **Expected**: `email` = `unknown-contact@nowhere.test`, `their_avg_reply_hours` = null, `your_avg_reply_hours` = null, `their_reply_count` = 0, `your_reply_count` = 0, `fastest_reply_hours` = null, `slowest_reply_hours` = null, `total_exchanges` = 0

## Success Criteria
- [ ] Response status is 200 (not 404)
- [ ] `their_avg_reply_hours` is null
- [ ] `your_avg_reply_hours` is null
- [ ] `their_reply_count` is 0
- [ ] `your_reply_count` is 0
- [ ] `fastest_reply_hours` is null
- [ ] `slowest_reply_hours` is null
- [ ] `total_exchanges` is 0

## Failure Criteria
- Response is 404 instead of 200 with zeros
- Any numeric field is non-zero for an unknown contact
- Average fields are 0 instead of null

## Notes
The API returns 200 with empty stats rather than 404 when no shared threads exist. This is the correct behavior — the contact is valid, there is just no exchange history to analyze.
