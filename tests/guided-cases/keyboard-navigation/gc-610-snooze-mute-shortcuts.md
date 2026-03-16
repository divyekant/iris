# GC-610: Snooze (b) and Mute (m) Shortcuts Act on Focused Message

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, shortcuts, snooze, mute, b-key, m-key
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app (not an input field)
### Data
- At least 2 emails in the inbox (source: local-db)
- Neither email is currently snoozed or muted

## Steps

1. Navigate to the inbox and focus a message with `j`
   - **Target**: Inbox view
   - **Input**: Press `j` once to move focus to a message row
   - **Expected**: A message row is visually focused

2. Press `b` to snooze the focused message
   - **Target**: Focused message row
   - **Input**: Press `b`
   - **Expected**: A snooze date/time picker or quick-snooze menu appears (e.g., "Later today", "Tomorrow morning", "Next week"), allowing the user to select a snooze duration

3. Select a snooze time and confirm
   - **Target**: Snooze picker
   - **Input**: Select "Tomorrow morning" (or the first available option) and confirm
   - **Expected**: The message is snoozed and removed from the inbox view; a confirmation toast or visual feedback indicates the snooze was set

4. Navigate to another message and press `m` to mute it
   - **Target**: Next focused message row (use `j` to focus)
   - **Input**: Press `m`
   - **Expected**: The thread is muted; future replies to this thread will not appear in the inbox. A confirmation toast or muted indicator appears. The message may optionally be removed from inbox view or marked with a muted badge.

5. Verify `b` and `m` do not fire inside an input field
   - **Target**: Search input
   - **Input**: Click into the search box, then press `b` and `m`
   - **Expected**: Characters "b" and "m" are typed into the search field; no snooze or mute action occurs

## Success Criteria
- [ ] `b` opens a snooze picker for the currently focused message
- [ ] Selecting a snooze duration removes the message from the inbox until the snooze expires
- [ ] `m` mutes the focused message's thread
- [ ] Muting suppresses future replies from appearing in the inbox
- [ ] Both shortcuts are suppressed when focus is inside a text input or contenteditable

## Failure Criteria
- `b` or `m` has no effect when pressed on a focused message
- Snooze picker does not appear after pressing `b`
- Mute does not suppress the thread from future inbox delivery
- Shortcuts fire inside the search input or compose area
