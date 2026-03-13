# GC-303: Mark reminder as acted upon

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, acted, resolved
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one follow-up reminder in `pending` status (note its `id`, e.g., `REMINDER_ID=3`)

## Steps

1. Mark the reminder as acted upon
   - **Target**: `PUT /api/followups/{id}/acted`
   - **Input**:
     ```bash
     curl -s -X PUT "http://localhost:3000/api/followups/${REMINDER_ID}/acted" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, response contains `status = "acted"`

2. Confirm the reminder is absent from the pending list
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the acted reminder's `id` is not in the returned array

3. Verify acted reminder is retrievable by status
   - **Target**: `GET /api/followups?status=acted`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=acted" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, the reminder appears with `status = "acted"` and an `acted_at` timestamp

## Success Criteria
- [ ] `PUT /api/followups/{id}/acted` returns 200
- [ ] Response shows `status = "acted"`
- [ ] `acted_at` timestamp is present and non-null
- [ ] Reminder does not appear in `?status=pending` list
- [ ] Reminder appears in `?status=acted` list

## Failure Criteria
- Non-200 response from acted endpoint
- `status` remains `"pending"` after the call
- `acted_at` is null or absent
- Reminder still appears in pending list

## Notes
The `acted` status signals the user replied or took action manually, resolving the follow-up. It is the positive resolution state, distinct from `dismiss` (user ignores) and `snoozed` (deferred). Recording `acted_at` allows tracking response cadence over time.
