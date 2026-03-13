# GC-306: Cannot snooze a dismissed reminder

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, snooze, state-machine, dismissed
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- A follow-up reminder that has already been dismissed (status = `"dismissed"`)
  - Either run GC-302 first to dismiss a reminder, or seed a dismissed reminder directly
  - Note the reminder's `id` as `DISMISSED_ID`

## Steps

1. Attempt to snooze an already-dismissed reminder
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${DISMISSED_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-20"}'
     ```
   - **Expected**: 409 Conflict (or 400 Bad Request) with an error message indicating the reminder cannot be snoozed in its current state

2. Attempt to snooze an already-acted reminder (bonus boundary)
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**: Use a reminder with `status = "acted"` (note its `id` as `ACTED_ID`)
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${ACTED_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-20"}'
     ```
   - **Expected**: 409 Conflict or 400 Bad Request — terminal states cannot be snoozed

3. Verify the dismissed reminder status is unchanged
   - **Target**: `GET /api/followups?status=dismissed`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=dismissed" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the reminder still appears with `status = "dismissed"` — it was not changed to `"snoozed"`

## Success Criteria
- [ ] Snoozing a dismissed reminder returns 409 or 400
- [ ] Snoozing an acted reminder returns 409 or 400
- [ ] Dismissed reminder `status` remains `"dismissed"` after failed snooze attempt
- [ ] No 500 errors

## Failure Criteria
- Dismissed reminder successfully transitions to `"snoozed"` (invalid state transition)
- Server returns 500 instead of 4xx
- Reminder status changes to `"snoozed"` despite rejection response

## Notes
The follow-up reminder lifecycle has terminal states: `dismissed` and `acted`. The state machine must reject any further transitions from these states. This prevents a dismissed reminder from being accidentally re-activated via a snooze call.
