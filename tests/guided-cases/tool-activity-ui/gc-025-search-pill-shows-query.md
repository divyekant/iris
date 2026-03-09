# GC-025: Search Tool Pill Shows Query Text

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: tool-activity-ui
- **Tags**: v11, ui, tool-activity, search, query-text
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3001
- AI provider configured and enabled
- Browser with access to the app

### Data
- Emails exist in the database (source: local-db)

## Steps

1. Open the AI Chat panel
   - **Target**: Click the AI Chat button in the top navigation bar
   - **Expected**: Chat panel opens

2. Send a search-triggering message
   - **Target**: Chat input field
   - **Input**: Type "Find emails about project deadline" and press Enter
   - **Expected**: Loading state appears

3. Wait for AI response and check search pill
   - **Target**: Tool activity pills in the assistant message
   - **Expected**: A search pill appears with magnifying glass icon and text "Searched: " followed by the search query terms (e.g., "Searched: project deadline" or similar keywords)

## Success Criteria
- [ ] Search pill includes the search query text after "Searched:"
- [ ] The query text reflects keywords from the user's message
- [ ] Magnifying glass icon is visible in the pill

## Failure Criteria
- Search pill shows no query text (just "Searched:" with nothing after)
- Search pill shows raw JSON instead of the query string
- No search pill visible

## Notes
- The pill reads `tc.arguments?.query` from the tool call record to display the search terms
- The AI extracts relevant keywords, so the exact query may differ from the user's message
