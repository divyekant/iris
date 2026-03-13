# GC-245: Same-sender consecutive messages — no reply counted

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: response-times
- **Tags**: response-times, edge-case, same-sender, consecutive, algorithm
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account (source: prior sync)
- A thread where the contact sends multiple messages in a row before you reply (source: seed data with known message ordering)
  - Example: Contact sends M1 at t=0, M2 at t=1h, you reply M3 at t=3h
  - M1→M2 should NOT count as a reply (same sender)
  - M2→M3 SHOULD count as your reply (sender changed)

## Steps
1. Request response times for a contact with consecutive same-sender messages
   - **Target**: `GET /api/contacts/{email}/response-times`
   - **Input**: email = contact with known consecutive same-sender messages in a thread
   - **Expected**: 200 OK with reply counts reflecting only sender-change transitions

2. Verify reply count accuracy
   - **Target**: Response JSON inspection
   - **Input**: Compare reply counts against known thread structure
   - **Expected**: Reply counts match the number of actual sender-change transitions, not the total number of message pairs

## Success Criteria
- [ ] Response status is 200
- [ ] Consecutive messages from the same sender are NOT counted as replies
- [ ] Only sender-change transitions produce reply delta calculations
- [ ] `their_reply_count` + `your_reply_count` <= total message pairs (strict less-than when consecutive same-sender exists)

## Failure Criteria
- Every consecutive message pair counted as a reply regardless of sender
- Reply counts exceed the number of sender-change transitions
- Average reply hours skewed by including same-sender time deltas

## Notes
This is the core algorithmic correctness test. The response-time algorithm must compare `from_address` between consecutive messages and only count a reply when the sender changes. If M1 and M2 are from the same person, M1→M2 is a follow-up, not a reply.
