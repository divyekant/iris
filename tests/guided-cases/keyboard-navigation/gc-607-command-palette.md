# GC-607: Command Palette Opens, Filters, Executes, and Closes

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, command-palette, cmd-k, shortcuts
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app window
### Data
- At least one email account connected and inbox loaded
- Commands available: navigate, compose, archive, change settings

## Steps

1. Open the command palette with Cmd+K
   - **Target**: Iris app with inbox visible
   - **Input**: Press `Cmd+K` (macOS) or `Ctrl+K` (Linux/Windows)
   - **Expected**: Command palette modal opens, centered on screen with a search input focused and a list of available commands visible

2. Type a search query to filter commands
   - **Target**: Command palette search input
   - **Input**: Type "compose"
   - **Expected**: Command list is filtered in real time to show only commands matching "compose" (e.g., "New Message", "Compose"); non-matching commands are hidden

3. Select a command with the keyboard and execute it
   - **Target**: Filtered command list
   - **Input**: Press `Enter` on the highlighted (first) result
   - **Expected**: Command executes (e.g., compose modal opens); command palette closes immediately after execution

4. Reopen the palette and close it with Escape
   - **Target**: Iris app
   - **Input**: Press `Cmd+K` to reopen the palette, then press `Escape`
   - **Expected**: Command palette opens, then closes without executing any command; the app returns to its previous state

5. Verify the palette can also be dismissed by clicking outside
   - **Target**: Command palette modal overlay
   - **Input**: Click outside the palette dialog on the backdrop
   - **Expected**: Palette closes without executing any command

## Success Criteria
- [ ] `Cmd+K` opens the command palette from any view (inbox, thread, settings)
- [ ] Typing in the search input filters commands in real time
- [ ] `Enter` executes the highlighted command and closes the palette
- [ ] `Escape` closes the palette without executing a command
- [ ] Clicking the backdrop closes the palette without executing a command
- [ ] The search input receives focus automatically when the palette opens

## Failure Criteria
- `Cmd+K` has no effect or opens the browser's address bar instead of the palette
- Command list does not filter as the user types
- Pressing `Enter` does nothing or does not close the palette
- `Escape` executes a command instead of dismissing
- Palette remains open after a command is executed
