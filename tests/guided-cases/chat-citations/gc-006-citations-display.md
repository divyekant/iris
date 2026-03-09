# GC-006: AI response includes email citations

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui+api
- **Flow**: chat-citations
- **Tags**: chat, citations, references, display
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured and synced
- AI provider enabled and healthy (Settings > AI shows a green status)
- Memories MCP running at localhost:8900 (or FTS5 fallback available)

### Data
- Inbox contains multiple emails from identifiable senders (e.g., Google, LinkedIn, GitHub) so that a sender-specific query yields results
- Emails have been indexed in FTS5 (synced via IMAP) and optionally stored in Memories (source: iris/{account}/messages/{id})

## Steps

1. Open the Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible, inbox displayed

2. Open AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel slides in from the right with empty state showing "Ask me anything about your inbox" and suggestion pills

3. Ask a sender-specific question
   - **Target**: Chat input field (placeholder "Ask about your inbox...")
   - **Input**: "What emails did I get from Google?"
   - **Expected**: User message bubble appears on the right, loading dots animate on the left

4. Observe the AI response
   - **Target**: Assistant message bubble (left-aligned, bg-surface background)
   - **Expected**: Response content references specific Google emails by describing their subjects or content

5. Verify the "References:" section below the response
   - **Target**: Citation block below the assistant message content, separated by a border-top
   - **Expected**: A "References:" label with an envelope icon (SVG) appears, followed by one or more citation lines

6. Verify each citation format
   - **Target**: Individual citation lines within the References section
   - **Expected**: Each citation displays in "{from}: {subject}" format, styled in text-muted color at 11px font size, truncated with CSS if too long

7. Verify the API response structure
   - **Target**: Network tab > POST /api/ai/chat response body
   - **Expected**: Response JSON has shape `{ "message": { "id": "...", "content": "...", "citations": [ { "message_id": "...", "subject": "...", "from": "...", "snippet": "..." } ], ... } }` where citations is a non-empty array

## Success Criteria
- [ ] Assistant message bubble is displayed with substantive content about Google emails
- [ ] "References:" section appears below the message content with envelope icon
- [ ] At least one citation is rendered in "{from}: {subject}" format
- [ ] Citations correspond to actual emails in the inbox (not fabricated)
- [ ] API response `message.citations` is a non-empty array with `message_id`, `subject`, `from`, and `snippet` fields
- [ ] Only citations whose message_id (first 8 chars) appears in the AI response text are included (filtered server-side)
- [ ] Citation text is truncated with CSS when it overflows the message bubble width

## Failure Criteria
- AI response has no "References:" section despite relevant emails existing in the inbox
- Citations array is present in API but UI does not render the References block
- Citations show "null" or "undefined" for from/subject fields
- Citations reference message IDs that do not exist in the messages table
- API returns an error status (4xx/5xx) instead of a chat response
