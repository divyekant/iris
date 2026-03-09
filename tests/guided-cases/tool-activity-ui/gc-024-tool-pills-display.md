# GC-024: Tool Activity Pills Display After AI Response

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: tool-activity-ui
- **Tags**: v11, ui, tool-activity, pills, ChatPanel
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3001
- AI provider configured and enabled
- Browser with access to the app

### Data
- At least one email exists in the database (source: local-db)

## Steps

1. Navigate to the Iris app
   - **Target**: http://localhost:3001
   - **Expected**: App loads, inbox visible

2. Open the AI Chat panel
   - **Target**: Click the AI Chat button in the top navigation bar
   - **Expected**: Chat panel slides open on the right side

3. Send a message that triggers tool use
   - **Target**: Chat input field
   - **Input**: Type "Search for emails about meetings" and press Enter
   - **Expected**: Loading indicator appears with animated dots and "Thinking..." text

4. Wait for AI response
   - **Target**: Chat message area
   - **Expected**: AI response appears with tool activity pills above the message text

5. Verify tool activity pills are visible
   - **Target**: The assistant message bubble
   - **Expected**: One or more small colored pills appear above the message text, showing which tools were used (e.g., magnifying glass icon + "Searched: meetings")

## Success Criteria
- [ ] Tool activity pills appear above the AI response text
- [ ] Pills have the branded primary color styling (gold/amber tint)
- [ ] At least one pill shows a tool name with an icon

## Failure Criteria
- No tool activity pills visible despite tools being used
- Pills appear below the message text instead of above
- Pills are unstyled or use wrong colors

## Notes
- Tool pills use `color-mix(in srgb, var(--iris-color-primary) 12%, transparent)` background
- Each tool type has a unique SVG icon: magnifying glass (search), envelope (read), list (list_emails), bar chart (inbox_stats)
