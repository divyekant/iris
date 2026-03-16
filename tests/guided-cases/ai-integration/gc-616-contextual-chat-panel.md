# GC-616: Contextual Chat Panel Shows "Chatting About: [Subject]" When Opened from ThreadView

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: thread-view, chat-panel, context, ai-chat
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Ollama or another AI provider is running and reachable
### Data
- At least one thread with a readable subject line (not empty)

## Steps

### Step 1: Navigate to inbox and open a thread
- **Target**: Thread row in inbox
- **Input**: Click a thread with a clear subject (e.g., "Project Alpha Kickoff")
- **Expected**: ThreadView opens showing the thread

### Step 2: Open the AI Chat panel from ThreadView
- **Target**: AI Chat button/icon in the thread action bar or top nav
- **Input**: Click the Chat button while in ThreadView
- **Expected**: ChatPanel slides open; panel is visible alongside or over the thread

### Step 3: Observe the chat panel header
- **Target**: ChatPanel header area
- **Input**: No interaction — observe on load
- **Expected**: Header or subtitle shows "Chatting about: [thread subject]" (e.g., "Chatting about: Project Alpha Kickoff")

### Step 4: Open chat from inbox (no thread context)
- **Target**: AI Chat button from the inbox view (not inside a thread)
- **Input**: Navigate back to inbox, then open Chat
- **Expected**: ChatPanel opens without "Chatting about:" indicator (or shows a generic context label like "Chatting about: your inbox")

### Step 5: Return to thread and re-open chat
- **Target**: Same thread as Step 1
- **Input**: Click thread to open ThreadView, then open Chat again
- **Expected**: "Chatting about: [subject]" reappears, confirming context is set per-thread-view

## Success Criteria
- [ ] Chat panel opened from ThreadView shows "Chatting about: [thread subject]"
- [ ] Subject text matches the actual thread subject
- [ ] Chat panel opened from inbox does not show thread-specific context label
- [ ] Context label updates correctly when switching between threads

## Failure Criteria
- "Chatting about" indicator absent from ChatPanel when opened from ThreadView
- Subject text is wrong, empty, or shows a thread ID instead of subject
- Context label persists incorrectly when chat is opened from inbox
- ChatPanel fails to open
