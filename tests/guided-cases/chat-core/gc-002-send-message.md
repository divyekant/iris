# GC-002: Send a Message and Receive AI Response

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui, api
- **Flow**: chat-core
- **Tags**: chat, send, response, enter-key
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured with synced emails
- AI provider enabled and available (Ollama, Anthropic, or OpenAI)
- Session token auto-bootstrapped on page load

### Data
- At least a few emails synced in the inbox so the AI has context to reference

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in from the right side with empty state

3. Click the input field and type "What emails did I get today?"
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Expected**: Text appears in the input field; Send button becomes enabled (gold colored)

4. Press the Enter key to send the message
   - **Target**: Keyboard Enter key
   - **Expected**: User message "What emails did I get today?" appears right-aligned in the chat panel

5. Observe loading state
   - **Target**: ChatPanel message area
   - **Expected**: Three bouncing dots animation appears below the user message, indicating the AI is processing

6. Wait for AI response
   - **Target**: ChatPanel message area
   - **Expected**: Loading dots disappear; assistant response appears left-aligned below the user message

## Success Criteria
- [ ] User message "What emails did I get today?" is displayed right-aligned in the chat
- [ ] Loading indicator (3 bouncing dots) appears while waiting for response
- [ ] Assistant response appears left-aligned after loading completes
- [ ] Response content references emails or inbox information (not a generic error)
- [ ] Input field is cleared after the message is sent
- [ ] Input field is re-focused and ready for the next message
- [ ] POST /api/ai/chat was called with { session_id, message } payload

## Failure Criteria
- Message does not appear in the chat after pressing Enter
- Loading dots never appear or never stop
- AI response is missing, empty, or shows an error (e.g., 503 provider unavailable)
- Input field retains the sent text after submission
- Network request to /api/ai/chat fails or is not made
