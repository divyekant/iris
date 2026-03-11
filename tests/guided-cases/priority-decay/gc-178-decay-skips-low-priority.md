# GC-178: Decay Skips Low-Priority Messages (score <= 0.3)

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, skip, low-priority, score, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- Two messages older than `decay_threshold_days` (7 days):
  - Message A: `ai_priority_score` = 0.25 (at or below the 0.3 skip threshold — should be skipped)
  - Message B: `ai_priority_score` = 0.35 (above the 0.3 skip threshold — should be decayed)
- Decay configured: `decay_enabled=true`, `decay_threshold_days=7`, `decay_factor=0.85`

## Steps
1. Confirm decay is enabled and threshold settings
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `decay_enabled: true`, `decay_threshold_days: 7`, `decay_factor: 0.85`
2. Record Message A's current score (should be 0.25)
   - **Target**: GET /api/messages/{message_a_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is 0.25
3. Record Message B's current score (should be 0.35)
   - **Target**: GET /api/messages/{message_b_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is 0.35
4. After decay runs, re-fetch Message A
   - **Target**: GET /api/messages/{message_a_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is still 0.25 (unchanged — decay was skipped)
5. After decay runs, re-fetch Message B
   - **Target**: GET /api/messages/{message_b_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is approximately 0.30 (0.35 * 0.85 = 0.2975, clamped to floor 0.1 — but 0.2975 is above 0.1 so result is ~0.30)

## Success Criteria
- [ ] Message A (score 0.25, at or below 0.3) retains its original score after decay
- [ ] Message B (score 0.35, above 0.3) has its score reduced by the decay factor
- [ ] `ai_priority_label` for Message A is unchanged
- [ ] `ai_priority_label` for Message B is recalculated to reflect the new score

## Failure Criteria
- Message A's score is reduced (decay applied to a low-priority message)
- Message B's score is unchanged (decay not applied to an eligible message)
- Server error (500) during or after decay
