# GC-175: Decay Factor Range — Values Between 0 and 1 Accepted

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, validation, decay-factor
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Set decay_factor to a valid low value (0.5 = 50% reduction per cycle)
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_factor": 0.5}`
   - **Expected**: 200 OK
2. Verify the value persisted
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `decay_factor` equals `0.5`
3. Set decay_factor to a valid high value (0.99 = minimal reduction)
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_factor": 0.99}`
   - **Expected**: 200 OK
4. Verify the updated value persisted
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: `decay_factor` equals `0.99`
5. Attempt to set decay_factor to an invalid value (1.5, out of range)
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_factor": 1.5}`
   - **Expected**: 400 Bad Request
6. Attempt to set decay_factor to 0 (would zero out scores)
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_factor": 0}`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] PUT with 0.5 returns 200 and persists
- [ ] PUT with 0.99 returns 200 and persists
- [ ] PUT with 1.5 returns 400
- [ ] PUT with 0.0 returns 400

## Failure Criteria
- Out-of-range values (0, >=1) accepted without error
- Valid values (0.5, 0.99) rejected
- Server error (500)
