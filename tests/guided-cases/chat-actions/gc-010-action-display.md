# GC-010: Action Proposal Display Format

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-actions
- **Tags**: chat, action, proposal, display, ui
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- AI enabled in Settings (ai_enabled = "true") with at least one provider configured
- At least one email account configured with synced emails

### Data
- Inbox contains unread emails (source: synced via IMAP)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in from the right side with empty state

3. Type an action request into the chat input
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Input**: "Star my important unread emails"
   - **Expected**: Text appears in the input field

4. Send the message by clicking Send or pressing Enter
   - **Target**: Send button or Enter key
   - **Expected**: User message bubble appears right-aligned; loading indicator (bouncing dots) appears

5. Wait for AI response with action proposal
   - **Target**: ChatPanel messages area
   - **Expected**: Assistant message bubble appears left-aligned with the AI's natural-language description of what it would do

6. Inspect the action proposal UI (do NOT click Confirm)
   - **Target**: The `proposed_action` section within the assistant message bubble
   - **Expected**: Below the message text, separated by a top border (`border-t`), two elements are visible:
     - A description line (`text-xs font-medium`) showing what the action will do (e.g., "Star 3 important unread emails")
     - A "Confirm" button styled with gold background (`background: var(--iris-color-primary)`) and dark text (`color: var(--iris-color-bg)`), rounded corners (`rounded-lg`), using `text-xs font-medium`

## Success Criteria
- [ ] AI response includes a `proposed_action` object with `action`, `description`, and `message_ids` fields
- [ ] The `action` field is one of the valid actions: archive, delete, mark_read, mark_unread, star
- [ ] Description text is displayed above the Confirm button, inside a border-top separated section
- [ ] "Confirm" button uses the brand gold color (`--iris-color-primary`) as its background
- [ ] "Confirm" button text reads exactly "Confirm"
- [ ] The ACTION_PROPOSAL JSON is stripped from the visible message text (only the natural-language portion is shown)
- [ ] No action is executed without clicking Confirm (emails remain unchanged)

## Failure Criteria
- Raw ACTION_PROPOSAL JSON is visible in the message text
- No Confirm button is rendered despite the AI including an ACTION_PROPOSAL
- Confirm button uses incorrect styling (not gold, wrong font size)
- Description text is missing above the Confirm button
- The proposed_action section is not visually separated from the message content
