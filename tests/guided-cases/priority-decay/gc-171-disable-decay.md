# GC-171: PUT decay_enabled=false Disables Decay

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, update, disable
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- Decay currently enabled (set via PUT /api/config/ai with `{"decay_enabled": true}` if not already)

## Steps
1. Disable decay
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_enabled": false}`
   - **Expected**: 200 OK
2. Verify the setting persisted
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `decay_enabled` equal to `false`

## Success Criteria
- [ ] PUT response status is 200
- [ ] Subsequent GET returns `decay_enabled: false`
- [ ] Other config fields (decay_threshold_days, decay_factor) are unchanged

## Failure Criteria
- PUT returns non-200 status
- GET after PUT shows `decay_enabled: true` (change did not persist)
- Server error (500)
