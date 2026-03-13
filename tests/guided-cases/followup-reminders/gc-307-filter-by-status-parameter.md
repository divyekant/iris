# GC-307: Filter by status parameter

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, list, filter, status
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one reminder in each of: `pending`, `snoozed`, `dismissed`, `acted` status
  - Achieve by running GC-299 (pending), GC-301 (snoozed), GC-302 (dismissed), GC-303 (acted) first

## Steps

1. Filter by `status=snoozed`
   - **Target**: `GET /api/followups?status=snoozed`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=snoozed" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, all returned reminders have `status = "snoozed"`, none have `status = "pending"` or other values

2. Filter by `status=dismissed`
   - **Target**: `GET /api/followups?status=dismissed`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=dismissed" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, all returned reminders have `status = "dismissed"`

3. Filter by `status=acted`
   - **Target**: `GET /api/followups?status=acted`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=acted" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, all returned reminders have `status = "acted"`

4. Provide an unknown status value
   - **Target**: `GET /api/followups?status=unknown_value`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=unknown_value" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 400 Bad Request with error indicating invalid status value, OR empty array `[]` (graceful degradation)

5. Omit `status` parameter entirely
   - **Target**: `GET /api/followups`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, returns all active reminders (pending + snoozed-and-due); does not return dismissed or acted unless the API documents all-status default

## Success Criteria
- [ ] `?status=snoozed` returns only snoozed reminders
- [ ] `?status=dismissed` returns only dismissed reminders
- [ ] `?status=acted` returns only acted reminders
- [ ] Unknown status returns 400 or empty array — no 500 error
- [ ] No cross-contamination between status filter values

## Failure Criteria
- Filter returns reminders from multiple status values when only one is requested
- Unknown status returns 500
- Omitting status returns dismissed or acted reminders unexpectedly

## Notes
The status filter is the primary UI control for the Reminders page tabs (Pending, Snoozed, History). Each filter must be strict — a snoozed reminder must never appear in the pending tab. This test exercises all four status values and the boundary of an invalid value.
