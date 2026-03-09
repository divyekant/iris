# GC-044: Undo Send Delay Config — Set to 15 Seconds

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: config, undo-delay, settings, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- Current undo send delay value (source: `GET /api/config/undo-send-delay`)

## Steps

1. Read the current undo send delay
   - **Target**: `GET /api/config/undo-send-delay`
   - **Expected**: Returns `{ delay_seconds: N }` with a valid value (5-30)

2. Set the undo send delay to 15 seconds
   - **Target**: `PUT /api/config/undo-send-delay`
   - **Input**: `{ "delay_seconds": 15 }`
   - **Expected**: 200 response confirming the update

3. Read the undo send delay again to verify persistence
   - **Target**: `GET /api/config/undo-send-delay`
   - **Expected**: Returns `{ delay_seconds: 15 }`

4. Send a normal email and verify the delay is applied
   - **Target**: `POST /api/send`
   - **Input**: Valid email fields, no `schedule_at`
   - **Expected**: Response includes a pending send with `send_at` approximately `now + 15` seconds (epoch), `can_undo: true`

## Success Criteria
- [ ] `PUT /api/config/undo-send-delay` with `{ "delay_seconds": 15 }` returns success
- [ ] Subsequent `GET /api/config/undo-send-delay` returns `{ delay_seconds: 15 }`
- [ ] A normal send created after the config change has `send_at` approximately 15 seconds in the future
- [ ] The config change persists (not ephemeral)

## Failure Criteria
- `PUT` returns an error for a valid value (15)
- `GET` returns the old value after a successful `PUT`
- New sends do not reflect the updated delay
- Config resets to default after restart (if persistence is expected)

## Notes
The valid range for undo send delay is 5-30 seconds. This test uses 15 seconds as a mid-range value. After testing, consider restoring the original delay value to avoid side effects on other tests.
