# GC-011: Request Action Without Confirmable Emails

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-actions
- **Tags**: chat, action, empty, edge-case, no-match
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- AI enabled in Settings (ai_enabled = "true") with at least one provider configured
- At least one email account configured with synced emails

### Data
- Inbox does NOT contain any emails from "nobody@nonexistent.test" (source: assumption, fabricated address)

## Steps

1. Navigate to Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Click the "AI Chat" button in the top navigation
   - **Target**: Button with text "AI Chat" in TopNav
   - **Expected**: ChatPanel slides in from the right side with empty state

3. Type a request targeting a nonexistent sender
   - **Target**: Input field with placeholder "Ask about your inbox..."
   - **Input**: "Delete all emails from nobody@nonexistent.test"
   - **Expected**: Text appears in the input field

4. Send the message by clicking Send or pressing Enter
   - **Target**: Send button or Enter key
   - **Expected**: User message bubble appears right-aligned; loading indicator (bouncing dots) appears

5. Wait for AI response
   - **Target**: ChatPanel messages area
   - **Expected**: One of two acceptable outcomes:
     - **Outcome A (preferred):** AI responds with a message indicating no matching emails were found (e.g., "I couldn't find any emails from nobody@nonexistent.test") and does NOT include an ACTION_PROPOSAL. No Confirm button is shown.
     - **Outcome B (acceptable):** AI responds with an ACTION_PROPOSAL containing an empty `message_ids` array. A Confirm button may appear, but clicking it would result in `{ executed: false, updated: 0 }` from the backend (the `confirm_action` handler returns early when `message_ids` is empty).

6. If Outcome B — click the Confirm button (optional verification)
   - **Target**: Gold "Confirm" button (if present)
   - **Expected**: POST to `/api/ai/chat/confirm` succeeds but returns `{ executed: false, updated: 0 }`. No confirmation message ("Done! Updated ...") appears because the frontend only adds it when `res.executed` is true.

## Success Criteria
- [ ] AI does not hallucinate emails from the nonexistent sender
- [ ] Either: no ACTION_PROPOSAL is included and the AI explains no emails were found
- [ ] Or: ACTION_PROPOSAL has an empty `message_ids` array and Confirm does not claim to update any emails
- [ ] No database rows are modified (no emails archived, deleted, or changed)
- [ ] Chat remains functional after this interaction (user can send further messages)

## Failure Criteria
- AI fabricates email subjects or content from the nonexistent sender
- ACTION_PROPOSAL contains message_ids that do not correspond to emails from the requested sender
- Clicking Confirm (if shown) reports "Done! Updated N email(s)." with N > 0
- "Failed to execute action." error appears for a well-formed but empty action
- Chat panel crashes or becomes unresponsive
