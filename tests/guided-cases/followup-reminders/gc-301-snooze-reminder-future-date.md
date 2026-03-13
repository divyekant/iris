# GC-301: Snooze a reminder with future date

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, snooze
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one follow-up reminder in `pending` status (note its `id`, e.g., `REMINDER_ID=1`)

## Steps

1. Snooze the reminder until a future date
   - **Target**: `PUT /api/followups/{id}/snooze`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/snooze" \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"until": "2026-03-20"}'
     ```
   - **Expected**: 200 OK, response contains the updated reminder with `status = "snoozed"` and `snoozed_until = "2026-03-20"`

2. Verify the reminder no longer appears in the default pending list
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the snoozed reminder's `id` does not appear in the returned array

3. Confirm reminder appears when listing snoozed status
   - **Target**: `GET /api/followups?status=snoozed`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=snoozed" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the snoozed reminder appears with `status = "snoozed"` and `snoozed_until = "2026-03-20"`

## Success Criteria
- [ ] `PUT /api/followups/{id}/snooze` returns 200
- [ ] Response shows `status = "snoozed"`
- [ ] `snoozed_until` matches the submitted date `"2026-03-20"`
- [ ] Reminder does not appear in `?status=pending` list after snoozing
- [ ] Reminder appears in `?status=snoozed` list

## Failure Criteria
- Non-200 response from snooze endpoint
- `status` remains `"pending"` after snooze
- `snoozed_until` is null or incorrect
- Reminder still appears in pending list

## Notes
Snooze defers a reminder to a future date. The reminder should surface again once `snoozed_until` has passed (tested as a separate re-surfacing behavior). This case validates the immediate state transition and that the date is stored correctly.
