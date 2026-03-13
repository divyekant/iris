# GC-244: Contact with only sent messages — no replies counted

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, edge-case, one-direction, no-replies
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account (source: prior sync)
- A contact email where you have only sent messages to them but they never replied (source: seed data or real thread with outbound-only messages)

## Steps
1. Request response times for a one-way contact
   - **Target**: `GET /api/contacts/{email}/response-times`
   - **Input**: email = contact who has never replied (only received your messages)
   - **Expected**: 200 OK with null averages and zero reply counts

2. Verify reply counts are zero
   - **Target**: Response JSON inspection
   - **Input**: Check reply-specific fields
   - **Expected**: `their_reply_count` = 0, `their_avg_reply_hours` = null, `your_reply_count` = 0 (no incoming to reply to), `total_exchanges` reflects total messages in shared threads but reply metrics are zero/null

## Success Criteria
- [ ] Response status is 200
- [ ] `their_reply_count` is 0 (they never replied)
- [ ] `their_avg_reply_hours` is null
- [ ] `fastest_reply_hours` and `slowest_reply_hours` are null (no reply deltas exist)
- [ ] `total_exchanges` reflects thread message count (may be > 0)

## Failure Criteria
- Consecutive sent messages counted as replies (inflated your_reply_count)
- Non-null average hours when reply count is 0
- Division by zero error producing NaN or Infinity

## Notes
Tests the edge case where all messages in shared threads flow in one direction. The algorithm must not treat consecutive same-sender messages as replies. This also implicitly tests that division by zero is handled when computing averages.
