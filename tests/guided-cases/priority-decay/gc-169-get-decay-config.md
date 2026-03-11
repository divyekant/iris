# GC-169: GET /api/config/ai Returns Decay Settings

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: priority-decay
- **Tags**: priority-decay, config, read
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Fetch AI config
   - **Target**: GET /api/config/ai
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing keys `decay_enabled`, `decay_threshold_days`, and `decay_factor`

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `decay_enabled` (boolean)
- [ ] Response body contains `decay_threshold_days` (integer)
- [ ] Response body contains `decay_factor` (float)
- [ ] Response `Content-Type` is `application/json`

## Failure Criteria
- Response status is not 200
- Any of `decay_enabled`, `decay_threshold_days`, or `decay_factor` is absent from the response body
- Response body is not valid JSON
