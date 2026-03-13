# GC-302: Dismiss a reminder

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, dismiss
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one follow-up reminder in `pending` status (note its `id`, e.g., `REMINDER_ID=2`)

## Steps

1. Dismiss the reminder
   - **Target**: `PUT /api/followups/{id}/dismiss`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/dismiss" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, response contains `status = "dismissed"`

2. Verify dismissed reminder does not appear in the pending list
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the dismissed reminder's `id` is absent from the returned array

3. Verify dismissed reminder appears under dismissed status
   - **Target**: `GET /api/followups?status=dismissed`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=dismissed" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the reminder appears with `status = "dismissed"`

## Success Criteria
- [ ] `PUT /api/followups/{id}/dismiss` returns 200
- [ ] Response shows `status = "dismissed"`
- [ ] Reminder does not appear in `?status=pending` list after dismissal
- [ ] Reminder appears in `?status=dismissed` list

## Failure Criteria
- Non-200 response from dismiss endpoint
- `status` remains `"pending"` after dismissal
- Reminder still appears in the pending list
- 404 for a valid reminder ID

## Notes
Dismiss permanently removes a reminder from the user's active follow-up view without any re-surface date. This is distinct from snooze (temporary) and acted (resolved). The dismissed state is terminal — no further transitions expected.
