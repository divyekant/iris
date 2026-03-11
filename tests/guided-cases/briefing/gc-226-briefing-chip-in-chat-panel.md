# GC-226: Briefing — Chip in Chat Panel (UI Test)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: briefing
- **Tags**: briefing, ui, chat-panel, chip, skipped
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced

## Steps

1. Open the Chat Panel
   - **Target**: http://127.0.0.1:3000/#/ — click the chat toggle button
   - **Input**: n/a
   - **Expected**: Chat panel slides open; suggestion chips are visible

2. Locate the "Briefing" suggestion chip
   - **Target**: Chat panel suggestion chips area
   - **Input**: n/a
   - **Expected**: A chip labeled "Briefing" (or similar, e.g., "Daily Briefing") is visible among the suggestion chips

3. Click the Briefing chip
   - **Target**: The "Briefing" chip element
   - **Input**: Click
   - **Expected**: The chip text is sent as a message; the AI responds with a briefing summary

## Success Criteria
- [ ] A "Briefing" suggestion chip is visible in the chat panel
- [ ] Clicking the chip sends a message to the AI
- [ ] The AI response contains briefing-like content (summary, stats, or highlights)

## Failure Criteria
- No "Briefing" chip is present
- Clicking the chip does nothing
- The AI response is unrelated to briefing

## Status
- **Skipped**: UI-only test; requires browser interaction and visual verification
