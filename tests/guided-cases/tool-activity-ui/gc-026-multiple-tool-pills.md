# GC-026: Multiple Tool Activity Pills for Multi-Step Response

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: tool-activity-ui
- **Tags**: v11, ui, tool-activity, multi-tool, pills
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3001
- AI provider configured and enabled
- Browser with access to the app

### Data
- Multiple emails exist in the database from different senders (source: local-db)

## Steps

1. Open the AI Chat panel
   - **Target**: Click the AI Chat button in the top navigation bar
   - **Expected**: Chat panel opens

2. Send a complex question requiring multiple tools
   - **Target**: Chat input field
   - **Input**: Type "How many emails do I have total, and show me the most recent one in detail" and press Enter
   - **Expected**: Loading state with "Thinking..." appears

3. Wait for AI response and verify multiple pills
   - **Target**: Tool activity pills section in the assistant message
   - **Expected**: Two or more tool activity pills visible, representing different tools used (e.g., inbox_stats bar chart pill + search/list pill + read email envelope pill)

4. Verify pills wrap correctly
   - **Target**: Tool activity pills layout
   - **Expected**: Pills wrap to the next line if they don't fit in one row (flex-wrap behavior)

## Success Criteria
- [ ] Two or more distinct tool activity pills are visible
- [ ] Different icons are shown for different tool types
- [ ] Pills are arranged in a flex-wrap layout

## Failure Criteria
- Only one pill shown despite multiple tools being used
- Pills overflow and are hidden
- All pills show the same icon regardless of tool type

## Notes
- The pills container uses `flex flex-wrap gap-1` for proper wrapping
- Complex questions like "overview + detail" typically trigger inbox_stats + list_emails or search_emails + read_email
