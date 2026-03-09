# GC-017: Graceful Error Handling

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-errors
- **Tags**: chat, error, recovery, resilience
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

3. Send a normal message and verify successful exchange (baseline)
   - **Target**: Chat input field
   - **Input**: "Hello, what can you help me with?"
   - **Expected**:
     - User message bubble appears right-aligned
     - Loading dots appear, then assistant response arrives left-aligned
     - No error text is displayed anywhere in the message area
     - Error state variable is cleared (`error = ''` at start of `sendMessage`)

4. Verify no error indicator is present after successful exchange
   - **Target**: ChatPanel message area
   - **Expected**: No red text (styled with `var(--iris-color-error)`) visible in the chat; the `{#if error}` block is not rendered because `error` is an empty string

5. Send a long message (under 50,000 characters) to test resilience
   - **Target**: Chat input field
   - **Input**: A message of approximately 5,000 characters (e.g., repeat "Please summarize my inbox. " ~192 times to reach ~5,000 chars)
   - **Expected**:
     - User message bubble appears with the full long text (may overflow with scroll)
     - API processes the request — either returns a valid response or a handled error
     - If successful: assistant response appears, no error shown
     - If error: red centered error text appears saying "Failed to get response. Try again." (the generic catch-all in the `catch` block)
     - In either case, the app does not crash, freeze, or become unresponsive

6. Verify the chat remains functional after the long message
   - **Target**: Chat input field
   - **Input**: "What was that about?"
   - **Expected**:
     - Input field accepts text and is not stuck in a disabled state
     - Send button becomes enabled
     - Message sends successfully
     - If a previous error was displayed, it is cleared when this new message is sent (`error = ''` at the top of `sendMessage`)

7. Verify error clearing behavior
   - **Target**: ChatPanel message area
   - **Expected**: Any previously displayed error text is gone after the new message is sent; the `error` state is reset to `''` at the beginning of each `sendMessage` call

## Success Criteria
- [ ] Successful message exchange shows no error indicators
- [ ] Long message (~5,000 chars) does not crash the app or lock up the UI
- [ ] If the long message triggers an API error, the error text "Failed to get response. Try again." appears centered in red
- [ ] Error text uses `var(--iris-color-error)` color and is centered (`text-center`)
- [ ] After any error, sending a new message clears the previous error
- [ ] Input field and Send button return to normal state after error (loading is set to `false` in `finally` block)
- [ ] Chat conversation history is preserved after an error (user messages remain visible)

## Failure Criteria
- App crashes, freezes, or becomes unresponsive when processing a long message
- Error text does not clear when a new message is sent
- Input field remains permanently disabled after an error
- Loading dots animation persists indefinitely after an error (the `finally` block should always set `loading = false`)
- Chat message history is lost after an error
- Uncaught JavaScript exception visible in browser console
