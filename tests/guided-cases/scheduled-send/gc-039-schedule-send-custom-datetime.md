# GC-039: Schedule Send via Custom Datetime

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: scheduled-send
- **Tags**: schedule, custom-datetime, compose, datetime-picker
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- A valid recipient email address (source: manual input)
- A future datetime at least 1 hour from now (source: manually computed)

## Steps

1. Open the compose modal
   - **Target**: Inbox page > Compose button
   - **Expected**: ComposeModal opens with empty fields

2. Fill in email fields
   - **Target**: ComposeModal form fields
   - **Input**: To: valid recipient, Subject: "GC-039 Custom Schedule", Body: "Testing custom datetime schedule"
   - **Expected**: All fields populated, Send button enabled

3. Click the Clock icon to open SchedulePicker
   - **Target**: ComposeModal footer > Clock icon button
   - **Expected**: SchedulePicker dropdown appears with preset options and custom datetime picker

4. Select a custom datetime using the datetime picker
   - **Target**: SchedulePicker > custom datetime input
   - **Input**: A date and time at least 1 hour in the future (e.g., today + 2 hours, or a specific future date/time)
   - **Expected**: Custom datetime is accepted and displayed in the picker

5. Confirm the scheduled send
   - **Target**: SchedulePicker > Schedule Send button
   - **Expected**: ComposeModal closes or shows schedule confirmation bar, confirmation indicates the chosen custom datetime

6. Verify the scheduled send via API
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: Response includes an entry with `send_at` matching the custom datetime (epoch seconds), `scheduled: true`, correct subject/recipient

## Success Criteria
- [ ] Custom datetime picker allows selecting a future date and time
- [ ] Selected datetime is reflected accurately in the schedule confirmation
- [ ] `GET /api/send/scheduled` returns the entry with correct `send_at` timestamp
- [ ] The `send_at` epoch value matches the custom datetime chosen (within 60s tolerance for rounding)
- [ ] Entry has `scheduled: true` and `can_undo: false`

## Failure Criteria
- Custom datetime picker is not accessible or does not accept input
- Scheduled send has an incorrect `send_at` timestamp
- Entry does not appear in the scheduled sends list
- Timezone conversion causes the wrong time to be stored

## Notes
The custom datetime picker should enforce a minimum time in the future (at least > 5 seconds per the API contract). Verify that the datetime picker uses the user's local timezone and correctly converts to epoch seconds.
