# GC-004: Send Multiple Messages in Conversation

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui, api
- **Flow**: chat-core
- **Tags**: chat, conversation, history, scroll, multi-turn
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured with synced emails
- AI provider enabled and available (Ollama, Anthropic, or OpenAI)
- Session token auto-bootstrapped on page load

### Data
- At least a few emails synced in the inbox so the AI has context for multi-turn conversation

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in with empty state

3. Type "Summarize my unread emails" and press Enter
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Expected**: User message appears right-aligned; loading dots appear; AI responds with a summary left-aligned

4. Wait for the first AI response to complete
   - **Target**: ChatPanel message area
   - **Expected**: Assistant response fully rendered; loading dots gone; input field cleared and ready

5. Type "Which of those are urgent?" and press Enter
   - **Target**: Input field
   - **Expected**: Second user message appears right-aligned below the first exchange; loading dots appear; AI responds with urgency assessment referencing the prior summary

6. Wait for the second AI response to complete
   - **Target**: ChatPanel message area
   - **Expected**: Second assistant response appears left-aligned; conversation now shows 4 messages (2 user + 2 assistant) in chronological order

7. Type "Draft a reply to the most urgent one" and press Enter
   - **Target**: Input field
   - **Expected**: Third user message appears right-aligned; loading dots appear; AI responds with a draft reply or offers to compose one

8. Wait for the third AI response to complete
   - **Target**: ChatPanel message area
   - **Expected**: Third assistant response appears; conversation shows 6 messages total in correct order

9. Scroll up to verify earlier messages are still visible
   - **Target**: ChatPanel scrollable message area
   - **Expected**: All 6 messages (3 user + 3 assistant) are present and in the correct chronological order

## Success Criteria
- [ ] All three user messages appear right-aligned in chronological order
- [ ] All three assistant responses appear left-aligned in chronological order
- [ ] Each response arrives after its respective loading state
- [ ] Conversation history is maintained — the AI's second and third responses reference context from earlier messages
- [ ] The chat panel scrolls to keep the latest message visible as new messages arrive
- [ ] Scrolling up reveals all previous messages intact and unmodified
- [ ] Input field clears after each send and remains focused for the next message
- [ ] Each POST /api/ai/chat request includes the same session_id to maintain conversation context

## Failure Criteria
- Messages appear out of order or are duplicated
- Earlier messages disappear when new ones arrive
- AI responses do not reference prior conversation context (treats each message as isolated)
- Chat panel does not auto-scroll to show the latest message
- Input field retains text after sending or loses focus between messages
- Any request returns an error or the loading state hangs
