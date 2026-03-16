# GC-605: AI Dropdown Contains Summarize/Tasks/Generate Replies

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: grouped-action-bar
- **Tags**: thread-view, action-bar, dropdown, ai, summarize
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least one email thread with 2+ messages (source: local-db)

## Steps

1. Open a thread with multiple messages
   - **Target**: Click a multi-message thread from inbox
   - **Expected**: ThreadView loads

2. Click the "AI" dropdown trigger (sparkle icon)
   - **Target**: "AI" button with sparkle icon and chevron
   - **Expected**: Dropdown opens showing: Summarize, Extract Tasks, Generate Replies

3. Click "Summarize"
   - **Target**: Summarize item in dropdown
   - **Expected**: Dropdown closes. AI summary is generated and displayed (either in the thread intelligence strip or side panel, depending on implementation)

## Success Criteria
- [ ] AI dropdown contains exactly: Summarize, Extract Tasks, Generate Replies
- [ ] Clicking an AI action triggers the corresponding API call
- [ ] Dropdown closes after selecting an action

## Failure Criteria
- AI actions are missing from dropdown
- Dropdown stays open after action selection
