# GC-304: Scan with no sent emails returns zero reminders

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, ai, scan, empty-state
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- Account configured with no sent emails, OR all sent emails have already received replies
- No prior follow-up reminders in `pending` status

## Steps

1. Trigger a follow-up scan
   - **Target**: `POST /api/ai/scan-followups`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/scan-followups \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json"
     ```
   - **Expected**: 200 OK, response body contains `{ "scanned": 0, "created": 0 }` (or `scanned` > 0 but `created` = 0 if all sent emails have replies)

2. Verify the follow-up list is empty
   - **Target**: `GET /api/followups?status=pending`
   - **Input**:
     ```bash
     curl -s "http://localhost:3000/api/followups?status=pending" \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, response is an empty JSON array `[]`

## Success Criteria
- [ ] `POST /api/ai/scan-followups` returns 200
- [ ] `created` in scan response is 0
- [ ] `GET /api/followups?status=pending` returns 200 with `[]`
- [ ] No 500 errors or panics when scanning an empty sent folder

## Failure Criteria
- Scan returns non-200 status
- Scan returns `created > 0` despite no unreplied sent emails
- `GET /api/followups?status=pending` returns non-empty array
- Server panics or returns 500

## Notes
Edge case for the empty-state code path. The scan must complete gracefully and return a structured response with zeros rather than erroring. This is especially important during initial setup when no emails have been sent yet.
