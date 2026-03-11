# GC-177: Decay Does Not Reduce Score Below 0.1 Floor

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, floor, edge, score
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A message exists with `ai_priority_score` just above the floor after repeated decay — approximate this by using a score near 0.31 (above the 0.3 skip threshold, and one decay cycle at 0.85 puts it at ~0.26, which rounds toward floor territory)
- Decay configured: `decay_enabled=true`, `decay_threshold_days=7`, `decay_factor=0.85`
- The target message's `received_at` is at least 7 days in the past (so it qualifies for decay)

## Steps
1. Confirm decay settings
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `decay_enabled: true`, `decay_threshold_days: 7`, `decay_factor: 0.85`
2. Inspect the message's current priority score via the messages API
   - **Target**: GET /api/messages/{message_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is above 0.1 (e.g., 0.31)
3. Trigger decay manually or wait for the next hourly worker run, then re-fetch the message
   - **Target**: GET /api/messages/{message_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is >= 0.1 (never zero or below floor)
4. After repeated decay cycles (simulate by adjusting the message date far into the past), verify the floor holds
   - **Target**: GET /api/messages/{message_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `ai_priority_score` is exactly `0.1`, not lower; `ai_priority_label` is `low`

## Success Criteria
- [ ] Score never drops below 0.1 regardless of number of decay cycles applied
- [ ] When score would mathematically go below 0.1, it is clamped to exactly 0.1
- [ ] `ai_priority_label` is `low` when score is at the floor
- [ ] Score is not set to 0.0 or null

## Failure Criteria
- Score drops below 0.1
- Score becomes 0.0 or null after decay
- `ai_priority_label` is missing or incorrect at the floor
