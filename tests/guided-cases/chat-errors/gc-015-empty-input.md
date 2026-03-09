# GC-015: Empty Input Prevention

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: chat-errors
- **Tags**: chat, input, validation, empty, send-button
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured

### Data
- None required (source: inline)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Open the AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel slides in from the right with empty state, input field visible

3. Verify Send button is disabled with empty input
   - **Target**: Send button in ChatPanel input area
   - **Expected**: Send button has `disabled` attribute and `opacity-50` styling; the `disabled` condition is `loading || !input.trim()`

4. Press Enter with empty input field
   - **Target**: Chat input field (`<input>` with placeholder "Ask about your inbox...")
   - **Input**: Press Enter key (no text entered)
   - **Expected**: Nothing happens — `sendMessage()` returns early because `!msg` is truthy when input is empty; no message appears in the message area, no API call is made, no error is displayed

5. Click the Send button with empty input field
   - **Target**: Send button
   - **Input**: Mouse click
   - **Expected**: Click has no effect because the button is `disabled` (the `disabled={loading || !input.trim()}` binding prevents the click handler from firing); no message appears, no API call, no error

6. Type a single space and verify Send remains disabled
   - **Target**: Chat input field
   - **Input**: " " (single space character)
   - **Expected**: Send button remains disabled because `!input.trim()` evaluates to true for whitespace-only input

7. Clear the input and verify the chat area is still in empty state
   - **Target**: ChatPanel message area
   - **Expected**: "Ask me anything about your inbox" text and three suggestion pills are still displayed; no user or assistant messages in the conversation; no error text visible

## Success Criteria
- [ ] Send button is disabled when input is empty
- [ ] Send button is disabled when input contains only whitespace
- [ ] Pressing Enter with empty input does not send a message or trigger an API call
- [ ] Clicking disabled Send button does not send a message or trigger an API call
- [ ] No error message appears at any point during empty input attempts
- [ ] Empty state ("Ask me anything about your inbox") remains visible throughout
- [ ] Network tab in DevTools shows zero POST requests to `/api/ai/chat`

## Failure Criteria
- A message bubble appears in the chat area after pressing Enter or clicking Send with empty input
- An API call to `/api/ai/chat` is made with an empty message
- An error message is displayed
- The app crashes or shows unexpected behavior
- Loading dots animation appears
