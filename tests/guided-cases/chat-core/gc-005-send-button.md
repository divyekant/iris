# GC-005: Send Message via Send Button Click

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui, api
- **Flow**: chat-core
- **Tags**: chat, send, button, click
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
   - **Expected**: ChatPanel slides in with empty state

3. Verify the Send button is initially disabled or visually inactive
   - **Target**: Send button (gold colored) in the ChatPanel input area
   - **Expected**: Send button is present but disabled or visually muted when the input field is empty

4. Click the input field and type "Show me emails with attachments"
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Expected**: Text appears in the input field; Send button becomes enabled and displays the gold active color

5. Click the Send button (do NOT press Enter)
   - **Target**: Send button (gold colored) next to the input field
   - **Expected**: User message "Show me emails with attachments" appears right-aligned in the chat panel; input field is cleared

6. Observe loading state
   - **Target**: ChatPanel message area
   - **Expected**: Three bouncing dots animation appears below the user message

7. Wait for AI response
   - **Target**: ChatPanel message area
   - **Expected**: Loading dots disappear; assistant response appears left-aligned with information about emails containing attachments

## Success Criteria
- [ ] Send button is disabled or visually inactive when input field is empty
- [ ] Send button becomes enabled (gold color) when text is entered in the input field
- [ ] Clicking the Send button sends the message (same behavior as pressing Enter)
- [ ] User message appears right-aligned in the chat after clicking Send
- [ ] Loading indicator (3 bouncing dots) appears while waiting for response
- [ ] Assistant response appears left-aligned after loading completes
- [ ] Input field is cleared after the message is sent via button click
- [ ] POST /api/ai/chat was called with the correct payload

## Failure Criteria
- Clicking the Send button does nothing
- Send button remains disabled even when text is entered
- Message is sent but input field is not cleared
- Send button triggers a page reload or navigation instead of sending
- AI response fails or shows an error
