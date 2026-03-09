# GC-012: Start new session clears conversation

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: chat-sessions
- **Tags**: chat, session, new, clear, suggestions
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- AI provider configured and healthy (Ollama, Anthropic, or OpenAI)
- At least one email account configured with synced emails

### Data
- None required (source: inline)

## Steps

1. Open the AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel appears on the right with empty state -- "Ask me anything about your inbox" text and three suggestion pills ("Briefing", "Action items", "Unread summary")

2. Send a message to establish a conversation
   - **Target**: Chat input field (placeholder "Ask about your inbox...")
   - **Input**: "What are my latest emails about?"
   - **Expected**: User message bubble appears on the right. Loading dots animate. Assistant response bubble appears on the left with a reply.

3. Verify conversation has content
   - **Target**: ChatPanel messages area
   - **Expected**: At least two message bubbles visible (user + assistant). Empty state text and suggestion pills are no longer visible.

4. Click the "New" button in the chat header
   - **Target**: Button labeled "New" in the ChatPanel header (next to the close button)
   - **Expected**: All message bubbles disappear. The panel returns to the empty state with "Ask me anything about your inbox" text and the three suggestion pills.

5. Verify input field is cleared
   - **Target**: Chat input field
   - **Expected**: Input field is empty with placeholder text "Ask about your inbox..." visible. No error messages displayed.

6. Send a new message to confirm fresh session
   - **Target**: Chat input field
   - **Input**: "Summarize my unread emails"
   - **Expected**: User message bubble appears. Assistant responds. Only two message bubbles visible (from this new exchange), confirming the previous conversation is gone and this is a new session.

## Success Criteria
- [ ] Initial message sent and assistant response received
- [ ] Clicking "New" clears all message bubbles from the panel
- [ ] Empty state returns: "Ask me anything about your inbox" text visible
- [ ] All three suggestion pills reappear: "Briefing", "Action items", "Unread summary"
- [ ] Input field is empty after clicking "New"
- [ ] No error messages displayed after clicking "New"
- [ ] New message after reset produces a fresh conversation (previous messages do not reappear)
- [ ] New session uses a different sessionId (previous context is not carried over in AI responses)

## Failure Criteria
- Previous messages remain visible after clicking "New"
- Suggestion pills do not reappear after clearing conversation
- Input field retains text after clicking "New"
- Error message appears after clicking "New"
- New message after reset returns responses referencing the previous conversation context
