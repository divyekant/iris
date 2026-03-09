# GC-003: Use Suggestion Chip "Briefing"

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui, api
- **Flow**: chat-core
- **Tags**: chat, suggestion, chip, briefing
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured with synced emails
- AI provider enabled and available (Ollama, Anthropic, or OpenAI)
- Session token auto-bootstrapped on page load

### Data
- At least a few unread emails synced in the inbox so the briefing has content

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in with empty state showing "Ask me anything about your inbox" and three suggestion pills

3. Verify the three suggestion pills are displayed
   - **Target**: Suggestion pills area in the empty state
   - **Expected**: Three pills visible with text "Briefing", "Action items", "Unread summary"

4. Click the "Briefing" suggestion pill
   - **Target**: Pill/button with text "Briefing"
   - **Expected**: The suggestion sends automatically without requiring the user to press Enter; a user message appears right-aligned in the chat

5. Observe loading state
   - **Target**: ChatPanel message area
   - **Expected**: Three bouncing dots animation appears, indicating the AI is processing the briefing request

6. Wait for AI response
   - **Target**: ChatPanel message area
   - **Expected**: Loading dots disappear; assistant response appears left-aligned with a briefing summary of unread/recent emails

7. Verify suggestion pills are no longer displayed
   - **Target**: ChatPanel empty state area
   - **Expected**: The three suggestion pills ("Briefing", "Action items", "Unread summary") are no longer visible now that the conversation has started

## Success Criteria
- [ ] Clicking the "Briefing" pill sends the message automatically (no Enter key needed)
- [ ] A user message corresponding to the "Briefing" request appears right-aligned
- [ ] Loading indicator (3 bouncing dots) appears while the AI processes
- [ ] AI responds with a briefing that summarizes unread or recent emails
- [ ] Suggestion pills disappear from the chat panel after the first message is sent
- [ ] Input field remains available for follow-up messages
- [ ] POST /api/ai/chat was called with the briefing message payload

## Failure Criteria
- Clicking the pill does nothing or only fills the input without sending
- Suggestion pills remain visible after the conversation starts
- AI response is empty, generic, or shows an error
- Loading state does not appear or hangs indefinitely
