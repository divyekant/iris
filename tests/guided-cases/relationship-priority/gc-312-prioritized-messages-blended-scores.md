# GC-312: Prioritized Messages Returns Blended Scores

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, prioritized-messages, blended-score, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced account with messages in the inbox (source: prior sync)
- Relationship scores computed at least once (source: POST /api/ai/relationship-priority)
- At least some messages have AI priority scores already classified (source: background AI pipeline)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK, `scored` >= 0

3. Fetch prioritized messages for a known account
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id={account_id}&folder=INBOX`
   - **Input**: Header `X-Session-Token: {token}`; substitute a valid `account_id` from the accounts API
   - **Expected**: 200 OK with `{ "messages": [...], "total": N }` where N >= 1

4. Inspect message objects for required score fields
   - **Target**: Response JSON from step 3, first message object
   - **Input**: Check `relationship_score` and `blended_score` fields
   - **Expected**: Each message object has `relationship_score` (float in [0.0, 1.0] or null if no relationship data for sender) and `blended_score` (float in [0.0, 1.0])

5. Verify blended score formula adherence
   - **Target**: A message where both AI score and relationship score are non-null
   - **Input**: Compute expected blended = AI_score * 0.6 + relationship_score * 0.4
   - **Expected**: `blended_score` matches the expected value within floating-point tolerance (±0.001)

## Success Criteria
- [ ] Response status is 200
- [ ] Response contains `messages` array and `total` integer
- [ ] Each message has `relationship_score` and `blended_score` fields
- [ ] `blended_score` is in range [0.0, 1.0]
- [ ] Messages are ordered by `blended_score` descending (highest priority first)
- [ ] Blended score approximates 60% AI + 40% relationship when both are available

## Failure Criteria
- Non-200 status code
- `messages` or `total` missing from response
- Missing `relationship_score` or `blended_score` on message objects
- Messages not ordered by blended score
- Blended score outside [0.0, 1.0]

## Notes
The blended score formula is `0.6 * ai_priority + 0.4 * relationship_score`. If a sender has no relationship score (new contact), the relationship component should default to 0.0 rather than causing a null-propagation error.
