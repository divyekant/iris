# GC-041: List Scheduled Sends API Returns Only Future Sends

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: list, scheduled, filter, api, future-only
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- No pre-existing scheduled sends (or known baseline count) (source: API query)

## Steps

1. Create a scheduled send far in the future
   - **Target**: `POST /api/send`
   - **Input**: Valid email fields + `schedule_at` set to 2 hours from now (epoch seconds)
   - **Expected**: 200/201 response with `scheduled: true`, returns send `id` (call it `id_future`)

2. Create a normal (non-scheduled) send
   - **Target**: `POST /api/send`
   - **Input**: Valid email fields, no `schedule_at` field (or `schedule_at` omitted)
   - **Expected**: 200/201 response with `scheduled: false` (uses undo-send delay), returns send `id` (call it `id_normal`)

3. List scheduled sends
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: Response includes `id_future` but does NOT include `id_normal`

4. Verify filtering threshold
   - **Target**: Inspect response from step 3
   - **Expected**: All entries in the list have `send_at` > now + 30 seconds (the endpoint's threshold for distinguishing scheduled from undo-send items)

## Success Criteria
- [ ] `GET /api/send/scheduled` returns the future scheduled send (`id_future`)
- [ ] `GET /api/send/scheduled` does NOT return the normal undo-send item (`id_normal`)
- [ ] All returned entries have `send_at` more than 30 seconds in the future
- [ ] Each returned entry has `scheduled: true`

## Failure Criteria
- Normal (undo-send) items appear in the scheduled sends list
- The future scheduled send is missing from the list
- Entries with `send_at` <= now + 30s are included

## Notes
The 30-second threshold (`send_at > now + 30`) exists to distinguish scheduled sends from undo-send items which typically have `send_at` = now + undo_delay (5-30 seconds). This test verifies that filtering works correctly.
