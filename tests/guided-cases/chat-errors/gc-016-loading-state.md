# GC-016: Send Button and Input Disabled During Loading

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-errors
- **Tags**: chat, loading, disabled, animation, send-button
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured with synced emails
- AI provider configured and reachable (Ollama, Anthropic, or OpenAI)

### Data
- None required (source: inline)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Open the AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel slides in with empty state

3. Type a message in the chat input
   - **Target**: Chat input field (placeholder "Ask about your inbox...")
   - **Input**: "Summarize my unread emails"
   - **Expected**: Send button becomes enabled (no longer has `disabled` attribute or `opacity-50`)

4. Press Enter to send the message and immediately observe the loading state
   - **Target**: Chat input field
   - **Input**: Press Enter key
   - **Expected**:
     - User message bubble appears aligned right with text "Summarize my unread emails"
     - Input field is cleared (value set to `''` by `sendMessage`)
     - Input field becomes disabled (`disabled={loading}` where `loading` is now `true`), with `opacity-50` styling
     - Send button becomes disabled (`disabled={loading || !input.trim()}` — both conditions are true)
     - Loading dots animation appears: three bouncing dots (1.5x1.5 circles) in a left-aligned bubble with `bg-surface` background, each with staggered `animation-delay` (0ms, 150ms, 300ms)

5. Verify input cannot accept text during loading
   - **Target**: Chat input field
   - **Input**: Attempt to type "another message"
   - **Expected**: Input field does not accept keystrokes because it has the HTML `disabled` attribute

6. Verify Send button cannot be clicked during loading
   - **Target**: Send button
   - **Input**: Mouse click
   - **Expected**: Button click has no effect; even if it could fire, `sendMessage()` has an early return guard `if (!msg || loading) return`

7. Wait for AI response to arrive
   - **Target**: ChatPanel message area
   - **Expected**:
     - Loading dots disappear (`loading` set to `false` in `finally` block)
     - Assistant message bubble appears left-aligned with response content
     - Input field re-enables (no longer has `disabled` attribute, full opacity restored)
     - Send button remains disabled because input is empty (but no longer due to `loading`)

8. Type a new message to confirm input is functional again
   - **Target**: Chat input field
   - **Input**: "Thanks"
   - **Expected**: Input accepts text, Send button becomes enabled

## Success Criteria
- [ ] Input field is disabled during the API request (`disabled` attribute present)
- [ ] Send button is disabled during the API request
- [ ] Loading dots animation is visible with three bouncing dots in a left-aligned bubble
- [ ] Loading dots have staggered animation delays (0ms, 150ms, 300ms)
- [ ] Input field re-enables after response arrives
- [ ] Send button re-enables when new text is typed after response
- [ ] No duplicate messages are sent if user attempts rapid interaction during loading
- [ ] User message bubble appears immediately before the API call completes

## Failure Criteria
- Input field remains enabled during the API request, allowing additional messages
- Send button is clickable during loading, causing duplicate API calls
- Loading dots animation does not appear between sending and receiving
- Input field stays disabled after the response arrives
- Loading state persists indefinitely (timeout: 30 seconds indicates a likely provider issue, not a UI bug)
