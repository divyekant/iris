# GC-040: Cancel a Scheduled Send from Scheduled List

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: scheduled-send
- **Tags**: cancel, scheduled, api, delete
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- An existing scheduled send created via `POST /api/send` with `schedule_at` set to a future time (e.g., 1 hour from now) (source: API setup step)

## Steps

1. Create a scheduled send for setup
   - **Target**: `POST /api/send`
   - **Input**: Valid email fields + `schedule_at` set to 1 hour from now (epoch seconds)
   - **Expected**: 200/201 response with `scheduled: true`, response includes the send `id`

2. Verify the scheduled send exists
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: Response list includes the entry from step 1 with matching `id` and `status: "pending"`

3. Cancel the scheduled send
   - **Target**: `DELETE /api/send/scheduled/{id}` (using `id` from step 1)
   - **Expected**: 200 response confirming cancellation

4. Verify the scheduled send is no longer listed
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: Response list does NOT include the cancelled entry (or entry has `status: "cancelled"` and is filtered out)

## Success Criteria
- [ ] `POST /api/send` with future `schedule_at` returns a send with `scheduled: true`
- [ ] `GET /api/send/scheduled` initially includes the scheduled send
- [ ] `DELETE /api/send/scheduled/{id}` returns success
- [ ] After cancellation, the send no longer appears in `GET /api/send/scheduled`
- [ ] The underlying record has `status: "cancelled"`

## Failure Criteria
- `DELETE /api/send/scheduled/{id}` returns an error for a valid pending scheduled send
- Cancelled send still appears in the scheduled sends list after deletion
- Status is not updated to "cancelled" in the database

## Notes
Cancellation sets `status='cancelled'` rather than deleting the row, preserving an audit trail. The `GET /api/send/scheduled` endpoint should filter out cancelled entries.
