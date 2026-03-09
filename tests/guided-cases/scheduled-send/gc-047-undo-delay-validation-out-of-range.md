# GC-047: Undo Send Delay Validation — Reject Value Outside 5-30 Range

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: validation, config, undo-delay, negative, boundary, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- Current undo send delay value (source: `GET /api/config/undo-send-delay`)

## Steps

1. Read the current undo send delay to establish baseline
   - **Target**: `GET /api/config/undo-send-delay`
   - **Expected**: Returns `{ delay_seconds: N }` with a valid value (5-30)

2. Attempt to set delay below minimum (value: 2)
   - **Target**: `PUT /api/config/undo-send-delay`
   - **Input**: `{ "delay_seconds": 2 }`
   - **Expected**: 400 response with validation error indicating value must be >= 5

3. Attempt to set delay above maximum (value: 60)
   - **Target**: `PUT /api/config/undo-send-delay`
   - **Input**: `{ "delay_seconds": 60 }`
   - **Expected**: 400 response with validation error indicating value must be <= 30

4. Attempt to set delay to zero
   - **Target**: `PUT /api/config/undo-send-delay`
   - **Input**: `{ "delay_seconds": 0 }`
   - **Expected**: 400 response with validation error

5. Attempt to set delay to a negative value
   - **Target**: `PUT /api/config/undo-send-delay`
   - **Input**: `{ "delay_seconds": -5 }`
   - **Expected**: 400 response with validation error

6. Verify the original delay is unchanged
   - **Target**: `GET /api/config/undo-send-delay`
   - **Expected**: Returns the same value as step 1 (none of the invalid updates were applied)

## Success Criteria
- [ ] `PUT` with `delay_seconds: 2` returns 400 (below minimum)
- [ ] `PUT` with `delay_seconds: 60` returns 400 (above maximum)
- [ ] `PUT` with `delay_seconds: 0` returns 400
- [ ] `PUT` with `delay_seconds: -5` returns 400
- [ ] All error responses include a meaningful validation message
- [ ] `GET` after all failed attempts returns the original unchanged value
- [ ] No server errors (500) for any of these requests

## Failure Criteria
- Any out-of-range value is accepted (returns 200)
- The delay is changed to an invalid value
- Server returns 500 instead of 400 for invalid input
- Error messages are missing or non-descriptive

## Notes
The valid range for undo send delay is 5-30 seconds inclusive. Boundary values (5 and 30) should be accepted; this test only covers values outside the valid range. Consider also testing non-integer values (e.g., 10.5) and non-numeric values (e.g., "abc") if the API accepts JSON number types.
