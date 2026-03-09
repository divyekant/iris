# GC-001: Open AI Chat Panel

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: chat-core
- **Tags**: chat, panel, open
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured with synced emails

### Data
- None required (source: inline)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in from the right side, showing empty state with "Ask me anything about your inbox" text and 3 suggestion pills

## Success Criteria
- [ ] ChatPanel is visible on the right side of the screen
- [ ] Empty state text "Ask me anything about your inbox" is displayed
- [ ] Three suggestion pills visible: "Briefing", "Action items", "Unread summary"
- [ ] Input field with placeholder "Ask about your inbox..." is visible
- [ ] Send button is visible but disabled (no input yet)

## Failure Criteria
- ChatPanel does not appear after clicking AI Chat button
- App shows error or crashes
