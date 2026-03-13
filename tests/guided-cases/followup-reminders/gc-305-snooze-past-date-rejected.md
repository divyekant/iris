# GC-305: Snooze with past date is rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, snooze, validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one follow-up reminder in `pending` status (note its `id`, e.g., `REMINDER_ID=4`)

## Steps

1. Attempt to snooze a reminder with yesterday's date
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-12"}'
     ```
   - **Expected**: 400 Bad Request with an error message indicating the date must be in the future

2. Attempt to snooze with today's date (boundary)
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-13"}'
     ```
   - **Expected**: 400 Bad Request (today's date is not a valid future snooze target)

3. Attempt to snooze with a malformed date
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "not-a-date"}'
     ```
   - **Expected**: 400 Bad Request with error indicating invalid date format

4. Verify reminder status is unchanged after all failed attempts
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the reminder still appears with `status = "pending"`

## Success Criteria
- [ ] Past date returns 400 with a descriptive error
- [ ] Today's date returns 400
- [ ] Malformed date string returns 400
- [ ] Reminder `status` remains `"pending"` after all failed snooze attempts
- [ ] No 500 errors from any of the invalid inputs

## Failure Criteria
- Past date accepted and reminder moved to `"snoozed"` with an already-past `snoozed_until`
- Server returns 500 instead of 400
- Reminder status changed despite rejection

## Notes
Snoozing to a past date would cause the reminder to immediately re-surface (or never surface correctly), which defeats the purpose. The server must validate that `until` is strictly after the current date. Today's date edge case is included because a same-day snooze would appear to work but resolve immediately.
