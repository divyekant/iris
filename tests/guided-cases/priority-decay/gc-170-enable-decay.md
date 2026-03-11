# GC-170: PUT decay_enabled=true Persists

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, update, enable
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Set decay_enabled to true
   - **Target**: PUT /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`, Header `Content-Type: application/json`, Body `{"decay_enabled": true}`
   - **Expected**: 200 OK
2. Verify the setting persisted
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `decay_enabled` equal to `true`

## Success Criteria
- [ ] PUT response status is 200
- [ ] Subsequent GET returns `decay_enabled: true`
- [ ] Other config fields (decay_threshold_days, decay_factor) are unchanged

## Failure Criteria
- PUT returns non-200 status
- GET after PUT still shows `decay_enabled: false`
- Server error (500)
