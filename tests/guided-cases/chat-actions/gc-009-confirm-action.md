# GC-009: Request Action and Confirm

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: chat-actions
- **Tags**: chat, action, confirm, archive
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- AI enabled in Settings (ai_enabled = "true") with at least one provider configured
- At least one email account configured with synced emails

### Data
- Inbox contains emails from a recognizable sender (e.g., LinkedIn notifications) (source: synced via IMAP)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in from the right side with empty state

3. Type an action request into the chat input
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Input**: "Archive all emails from LinkedIn"
   - **Expected**: Text appears in the input field

4. Send the message by clicking Send or pressing Enter
   - **Target**: Send button or Enter key
   - **Expected**: User message bubble appears right-aligned; loading indicator (bouncing dots) appears while AI processes

5. Wait for AI response
   - **Target**: ChatPanel messages area
   - **Expected**: Assistant message bubble appears left-aligned containing a description of the proposed action (e.g., "I'll archive N emails from LinkedIn"). Below the description text, a gold "Confirm" button is rendered inside a border-top section. The button has `background: var(--iris-color-primary)` styling.

6. Click the "Confirm" button below the AI response
   - **Target**: Gold "Confirm" button within the assistant message's `proposed_action` section
   - **Expected**: POST request sent to `/api/ai/chat/confirm` with `{ session_id, message_id }`. On success, a new assistant message bubble appears: "Done! Updated N email(s)." where N is the number of emails affected.

## Success Criteria
- [ ] User message "Archive all emails from LinkedIn" is displayed in the chat
- [ ] AI response contains a description of the action it will perform
- [ ] A gold "Confirm" button appears below the AI's proposed action description
- [ ] Clicking Confirm triggers `POST /api/ai/chat/confirm` with correct session_id and message_id
- [ ] API returns `{ executed: true, updated: N }` where N >= 1
- [ ] Confirmation message "Done! Updated N email(s)." appears as a new assistant message
- [ ] The affected emails are moved to the Archive folder in the database

## Failure Criteria
- AI response does not include an ACTION_PROPOSAL (no Confirm button shown)
- Clicking Confirm produces "Failed to execute action." error
- API returns `{ executed: false, updated: 0 }` when matching emails exist
- Confirmation message never appears after clicking Confirm
- Chat panel crashes or shows unhandled exception
