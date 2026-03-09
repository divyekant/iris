# GC-013: Close and reopen chat panel

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: chat-sessions
- **Tags**: chat, session, close, reopen, state-reset
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
   - **Expected**: ChatPanel appears on the right with empty state and suggestion pills

2. Send a message to establish a conversation
   - **Target**: Chat input field
   - **Input**: "What action items do I have?"
   - **Expected**: User message bubble appears. Loading dots animate. Assistant response bubble appears with a reply.

3. Verify conversation is active
   - **Target**: ChatPanel messages area
   - **Expected**: At least two message bubbles visible (user + assistant). AI Chat button in TopNav is styled with primary background color (indicating panel is open).

4. Close the chat panel using the close button
   - **Target**: Close button (x) in the ChatPanel header
   - **Expected**: ChatPanel disappears. The AI Chat button in TopNav reverts to its inactive style (surface background with border). Main content area expands to fill the space.

5. Reopen the chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel appears fresh -- showing the empty state with "Ask me anything about your inbox" text and three suggestion pills. Previous conversation is not preserved because ChatPanel is destroyed and recreated (new sessionId generated via crypto.randomUUID() on mount).

6. Confirm the panel is fully reset
   - **Target**: ChatPanel
   - **Expected**: Input field is empty. No error messages. No loading indicator. Suggestion pills are visible and clickable.

## Success Criteria
- [ ] Message sent and response received in initial session
- [ ] Close button (x) hides the ChatPanel completely
- [ ] AI Chat button in TopNav toggles visual state (active when open, inactive when closed)
- [ ] Reopened panel shows empty state, not the previous conversation
- [ ] Suggestion pills ("Briefing", "Action items", "Unread summary") are visible on reopen
- [ ] Input field is empty on reopen
- [ ] No residual error messages or loading indicators on reopen

## Failure Criteria
- Previous conversation messages persist after close and reopen
- ChatPanel does not fully disappear when close button is clicked
- AI Chat button does not toggle its visual styling
- Error or loading state leaks across panel close/reopen cycle
- Panel reopens in a broken or partially rendered state
