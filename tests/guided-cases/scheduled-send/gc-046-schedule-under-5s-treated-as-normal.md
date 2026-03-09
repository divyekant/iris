# GC-046: Schedule Send for Time < 5s in Future Treated as Normal Send

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: edge-case, threshold, schedule, undo-send, api
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- Known undo send delay value (source: `GET /api/config/undo-send-delay`)

## Steps

1. Note the current time and undo delay
   - **Target**: System clock + `GET /api/config/undo-send-delay`
   - **Expected**: Current epoch time and undo delay (e.g., 10 seconds) are known

2. Send with `schedule_at` set to 3 seconds in the future
   - **Target**: `POST /api/send`
   - **Input**: Valid email fields + `schedule_at` = current epoch + 3
   - **Expected**: Since `schedule_at` (now + 3) is NOT > now + 5, the API treats this as a normal send, NOT a scheduled send

3. Inspect the response
   - **Target**: Response body from step 2
   - **Expected**: `scheduled: false`, `can_undo: true`, `send_at` = now + undo_delay (NOT the provided `schedule_at`)

4. Verify it does NOT appear in scheduled sends list
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: The send from step 2 is NOT in the scheduled sends list (it's an undo-send item, not a scheduled send)

## Success Criteria
- [ ] `POST /api/send` with `schedule_at` < now + 5s returns `scheduled: false`
- [ ] Response has `can_undo: true` (treated as normal undo-send)
- [ ] `send_at` in the response uses the undo delay, not the provided `schedule_at`
- [ ] The send does NOT appear in `GET /api/send/scheduled`

## Failure Criteria
- The API treats a near-future `schedule_at` as a scheduled send (`scheduled: true`)
- `can_undo` is false for what should be a normal send
- The send appears in the scheduled sends list
- The API returns an error instead of falling back to normal send behavior

## Notes
The 5-second threshold is the boundary between scheduled and normal sends. When `schedule_at` is provided but is less than 5 seconds in the future, the API should silently fall back to normal undo-send behavior. This prevents edge cases where a user schedules for "now" and bypasses the undo mechanism.
