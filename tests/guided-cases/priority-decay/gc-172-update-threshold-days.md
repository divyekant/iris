# GC-172: PUT decay_threshold_days=14 Updates Threshold

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, update, threshold
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Update the decay threshold to 14 days
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_threshold_days": 14}`
   - **Expected**: 200 OK
2. Verify the new threshold persisted
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `decay_threshold_days` equal to `14`

## Success Criteria
- [ ] PUT response status is 200
- [ ] Subsequent GET returns `decay_threshold_days: 14`
- [ ] `decay_enabled` and `decay_factor` are unchanged by this PUT

## Failure Criteria
- PUT returns non-200 status
- GET after PUT shows the old threshold value
- Other config fields are reset or altered unexpectedly
- Server error (500)
