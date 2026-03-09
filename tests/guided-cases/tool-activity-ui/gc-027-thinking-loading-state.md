# GC-027: Thinking Loading State During Tool Use

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: ui
- **Flow**: tool-activity-ui
- **Tags**: v11, ui, loading, thinking, animation
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

1. Open the AI Chat panel
   - **Target**: Click the AI Chat button in the top navigation bar
   - **Expected**: Chat panel opens

2. Send a message that triggers tool use
   - **Target**: Chat input field
   - **Input**: Type "Give me a briefing of my inbox" and press Enter
   - **Expected**: Loading state appears immediately

3. Observe loading indicator
   - **Target**: Below the messages area, before AI response appears
   - **Expected**: Three small animated bouncing dots in the brand primary color (gold/amber), with "Thinking..." text next to them in muted color

4. Verify input is disabled during loading
   - **Target**: Chat input field and Send button
   - **Expected**: Input field shows `disabled` state (opacity reduced), Send button is disabled and not clickable

5. Verify loading disappears when response arrives
   - **Target**: Chat message area
   - **Expected**: Bouncing dots and "Thinking..." text disappear, replaced by the AI response message with tool activity pills

## Success Criteria
- [ ] Three bouncing dots appear in brand primary color during loading
- [ ] "Thinking..." text appears next to the dots
- [ ] Input field and Send button are disabled during loading
- [ ] Loading state disappears when AI response arrives

## Failure Criteria
- No loading indicator shown during tool use
- Loading dots use wrong color (not brand primary)
- Input field remains enabled during loading (could cause double-send)
- Loading state persists after response arrives

## Notes
- The bouncing dots use CSS `animate-bounce` with staggered delays (0ms, 150ms, 300ms)
- Loading is especially noticeable during agentic tool use since multi-step responses take longer
- The `disabled` attribute on the input prevents sending another message while processing
