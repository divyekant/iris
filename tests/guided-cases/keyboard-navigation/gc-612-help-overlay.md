# GC-612: Help Overlay Shows Dynamic Shortcuts Grouped by Mode

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, help-overlay, question-mark, dynamic-shortcuts, grouped-by-mode
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app (not an input field)
### Data
- At least one email account connected; inbox loaded

## Steps

1. Open the help overlay from the inbox with `?`
   - **Target**: Iris app in inbox view
   - **Input**: Press `?`
   - **Expected**: A help overlay/modal opens showing a list of keyboard shortcuts, grouped by mode or context (e.g., "Inbox", "Thread", "Global")

2. Verify shortcuts are grouped by context
   - **Target**: Help overlay content
   - **Expected**: Shortcuts are organized into named sections. "Global" or "Anywhere" section contains commands like `?` (help), `Cmd+K` (command palette). "Inbox" section contains `j`/`k` (navigate), `e` (archive), `s` (star), `#` (delete), `b` (snooze), `m` (mute). "Navigation" section contains `g+i`, `g+s`, `g+d` chords.

3. Verify shortcut descriptions are human-readable
   - **Target**: Each shortcut entry in the overlay
   - **Expected**: Each entry shows a key binding (e.g., `j`) alongside a plain-language description (e.g., "Next message"). Chord shortcuts display both keys (e.g., `g` → `i`).

4. Close the overlay with Escape
   - **Target**: Help overlay
   - **Input**: Press `Escape`
   - **Expected**: The overlay closes; the app returns to the inbox view without any side effects

5. Open a thread, then open the help overlay and verify context-specific shortcuts appear
   - **Target**: Thread view
   - **Input**: Open a thread, then press `?`
   - **Expected**: The help overlay opens. The "Thread" section (if present) shows thread-specific shortcuts. Shortcuts not applicable to the current mode may be dimmed or absent.

6. Close the overlay by pressing `?` again or clicking outside
   - **Target**: Help overlay
   - **Input**: Press `?` or click the backdrop
   - **Expected**: Overlay closes

## Success Criteria
- [ ] `?` opens the help overlay from any keyboard-navigable view
- [ ] Shortcuts are visually grouped by mode/context (Inbox, Thread, Global, Navigation, etc.)
- [ ] Each shortcut entry shows both the key binding and a plain-language description
- [ ] `Escape` closes the overlay
- [ ] Clicking outside the overlay closes it
- [ ] Overlay reflects currently registered shortcuts (dynamic, not hardcoded)
- [ ] The overlay does not show duplicate entries for the same shortcut

## Failure Criteria
- `?` has no effect or opens browser search
- Overlay opens but shortcuts are not grouped (flat unorganized list)
- Any shortcut entry is missing its description
- `Escape` does not close the overlay
- Overlay shows stale or hardcoded shortcuts that do not match the centralized keyboard manager's current state
