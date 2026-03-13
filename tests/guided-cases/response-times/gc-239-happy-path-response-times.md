# GC-239: Happy path — response time stats returned for active contact

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, happy-path, stats, reply-timing
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account with messages (source: prior sync)
- A contact email with multiple back-and-forth exchanges in shared threads (source: seed or real inbox)

## Steps
1. Fetch response time stats for a known contact
   - **Target**: `GET /api/contacts/{email}/response-times`
   - **Input**: email = a contact with known reply exchanges (e.g., `alice@example.com`)
   - **Expected**: 200 OK, response body contains `email`, `their_avg_reply_hours`, `your_avg_reply_hours`, `their_reply_count`, `your_reply_count`, `fastest_reply_hours`, `slowest_reply_hours`, `total_exchanges`

2. Verify stat values are plausible
   - **Target**: Response JSON inspection
   - **Input**: Check numeric fields
   - **Expected**: `their_avg_reply_hours` and `your_avg_reply_hours` are non-negative floats or null, reply counts are non-negative integers, `fastest_reply_hours` <= `slowest_reply_hours` (when both non-null), `total_exchanges` >= `their_reply_count` + `your_reply_count`

## Success Criteria
- [ ] Response status is 200
- [ ] `email` field matches the queried contact
- [ ] `their_avg_reply_hours` and `your_avg_reply_hours` are present (non-null for active contact)
- [ ] `their_reply_count` and `your_reply_count` are positive integers
- [ ] `fastest_reply_hours` <= `slowest_reply_hours`
- [ ] `total_exchanges` is a positive integer

## Failure Criteria
- Non-200 status code
- Missing fields in response
- Negative values for hours or counts
- `fastest_reply_hours` > `slowest_reply_hours`

## Notes
Primary happy path. Confirms the full response-time analysis pipeline works end-to-end for a contact with real exchange history.
