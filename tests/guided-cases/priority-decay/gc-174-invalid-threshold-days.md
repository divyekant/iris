# GC-174: Invalid decay_threshold_days (0 or Negative) Handled

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, validation, negative, threshold
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- Note the current `decay_threshold_days` value via GET /api/config/ai (to confirm it is unchanged after rejections)

## Steps
1. Attempt to set threshold to 0
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_threshold_days": 0}`
   - **Expected**: 400 Bad Request
2. Attempt to set threshold to -5
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_threshold_days": -5}`
   - **Expected**: 400 Bad Request
3. Verify config is unchanged after the rejected attempts
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `decay_threshold_days` retains its pre-test value

## Success Criteria
- [ ] PUT with 0 returns 400
- [ ] PUT with -5 returns 400
- [ ] Config value is not mutated by the invalid requests
- [ ] Error response body contains a descriptive message (not empty)

## Failure Criteria
- Server accepts 0 or negative threshold (200 response)
- Config value is silently changed to 0 or a negative number
- Server error (500)
