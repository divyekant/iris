# GC-173: Decay Config Defaults — enabled=true, days=7

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, defaults, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- Config has not been explicitly overridden (fresh install or reset to defaults)
  - If uncertain, reset: PUT /api/config/ai with `{"decay_enabled": true, "decay_threshold_days": 7, "decay_factor": 0.85}`

## Steps
1. Fetch AI config and inspect defaults
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `decay_enabled: true`, `decay_threshold_days: 7`, `decay_factor: 0.85`

## Success Criteria
- [ ] Response status is 200
- [ ] `decay_enabled` defaults to `true`
- [ ] `decay_threshold_days` defaults to `7`
- [ ] `decay_factor` defaults to `0.85`
- [ ] All three fields are present and correctly typed (boolean, integer, float)

## Failure Criteria
- Any default value differs from the documented defaults (enabled=true, days=7, factor=0.85)
- Fields are absent or null
- Response status is not 200
