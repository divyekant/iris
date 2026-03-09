# GC-042: Schedule with Past Datetime Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: ui
- **Flow**: scheduled-send
- **Tags**: validation, past-datetime, negative, schedule-picker
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- A valid recipient email address (source: manual input)
- A past datetime value (source: manually chosen, e.g., yesterday at 10:00 AM)

## Steps

1. Open the compose modal
   - **Target**: Inbox page > Compose button
   - **Expected**: ComposeModal opens with empty fields

2. Fill in email fields
   - **Target**: ComposeModal form fields
   - **Input**: To: valid recipient, Subject: "GC-042 Past Schedule", Body: "Should be rejected"
   - **Expected**: All fields populated

3. Click the Clock icon to open SchedulePicker
   - **Target**: ComposeModal footer > Clock icon button
   - **Expected**: SchedulePicker dropdown appears

4. Attempt to select a past datetime in the custom picker
   - **Target**: SchedulePicker > custom datetime input
   - **Input**: A datetime in the past (e.g., yesterday at 10:00 AM)
   - **Expected**: The UI either (a) prevents selecting a past date (input disabled/greyed for past dates), or (b) shows a validation error when attempting to confirm

5. Attempt to confirm the schedule (if the UI allowed the selection)
   - **Target**: SchedulePicker > Schedule Send button
   - **Expected**: Validation error is displayed, scheduled send is NOT created

## Success Criteria
- [ ] SchedulePicker prevents scheduling emails for past datetimes
- [ ] Either the datetime picker disallows past date selection, or a clear validation error is shown
- [ ] No scheduled send is created in the system (verifiable via `GET /api/send/scheduled`)
- [ ] The compose modal remains open so the user can correct the datetime

## Failure Criteria
- A past datetime is accepted without any warning or error
- A scheduled send is created with a `send_at` in the past
- The UI silently fails or closes the compose modal without feedback

## Notes
The UI should enforce the constraint client-side (preventing past date selection in the datetime picker is the ideal UX). The API also enforces `schedule_at > now + 5s`, but this test focuses on the UI validation layer.
