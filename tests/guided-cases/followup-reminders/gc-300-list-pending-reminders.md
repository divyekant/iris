# GC-300: List pending follow-up reminders

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, list, pending
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one follow-up reminder in `pending` status already exists (run GC-299 or seed directly)

## Steps

1. List all follow-up reminders with default parameters
   - **Target**: `GET /api/followups`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, JSON array of reminder objects

2. List reminders filtered by `status=pending`
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, JSON array containing only reminders with `status = "pending"`

3. Apply default limit
   - **Target**: `GET /api/followups?limit=20`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?limit=20" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, array with at most 20 elements

4. Verify field completeness on first reminder
   - **Target**: First element from Step 2 response
   - **Input**: Inspect all fields
   - **Expected**: `id`, `message_id`, `thread_id`, `subject`, `recipient`, `sent_at`, `due_at`, `status` = `"pending"`, `urgency`, `created_at` are all present and non-null (except `thread_id` which may be null for single messages)

## Success Criteria
- [ ] Default `GET /api/followups` returns 200 with a non-empty array
- [ ] `?status=pending` filter returns only reminders where `status == "pending"`
- [ ] `?limit=20` caps result count at 20
- [ ] All required fields are present on each reminder
- [ ] Dismissed or snoozed reminders do not appear when `status=pending` is applied

## Failure Criteria
- Response is not 200
- `?status=pending` returns reminders with `status != "pending"`
- More than 20 results returned when `limit=20` is specified
- Any reminder is missing a required field

## Notes
Core list endpoint. Verifies the default view a user sees in the Reminders section — pending items only, capped at 20. The `limit` default of 20 guards against unbounded result sets.
