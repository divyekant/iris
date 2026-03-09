# GC-038: Schedule Send via Preset "Tomorrow Morning"

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: scheduled-send
- **Tags**: schedule, preset, tomorrow-morning, compose
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- A valid recipient email address (source: manual input)
- Current local time noted for verifying scheduled time calculation (source: system clock)

## Steps

1. Open the compose modal
   - **Target**: Inbox page > Compose button
   - **Expected**: ComposeModal opens with empty fields

2. Fill in email fields
   - **Target**: ComposeModal form fields
   - **Input**: To: valid recipient, Subject: "GC-038 Scheduled Test", Body: "Testing preset schedule"
   - **Expected**: All fields populated, Send button enabled

3. Click the Clock icon to open SchedulePicker
   - **Target**: ComposeModal footer > Clock icon button
   - **Expected**: SchedulePicker dropdown appears with preset options and custom datetime picker

4. Select "Tomorrow morning 8am" preset
   - **Target**: SchedulePicker > "Tomorrow morning" option
   - **Expected**: Preset is highlighted/selected, schedule time resolves to tomorrow at 08:00 local time

5. Confirm the scheduled send
   - **Target**: SchedulePicker > Schedule Send button (or equivalent confirm action)
   - **Expected**: ComposeModal closes (or shows schedule confirmation bar), toast/notification confirms the email is scheduled for tomorrow 8:00 AM

6. Verify the scheduled send appears in the API
   - **Target**: `GET /api/send/scheduled`
   - **Expected**: Response includes an entry with `scheduled: true`, `send_at` matching tomorrow 08:00 local time (epoch seconds), and the correct subject/recipient

## Success Criteria
- [ ] SchedulePicker opens from Clock icon in ComposeModal footer
- [ ] "Tomorrow morning" preset resolves to next day 08:00 local time
- [ ] After scheduling, a confirmation is shown to the user
- [ ] `GET /api/send/scheduled` returns the newly scheduled message
- [ ] The scheduled entry has `scheduled: true` and `can_undo: false`
- [ ] `send_at` timestamp corresponds to tomorrow 08:00 AM local time

## Failure Criteria
- SchedulePicker does not open or shows no preset options
- Preset time resolves to an incorrect timestamp (wrong day or wrong hour)
- Scheduled send does not appear in `GET /api/send/scheduled`
- Entry has `scheduled: false` or `can_undo: true`

## Notes
"Tomorrow morning" should always resolve to the next calendar day at 08:00 in the user's local timezone, regardless of current time of day.
