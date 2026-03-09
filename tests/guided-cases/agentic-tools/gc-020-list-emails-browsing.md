# GC-020: AI Uses List Emails for Browsing Request

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, list_emails, tool-use, filters
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Emails exist with varying dates and read status (source: local-db)

## Steps

1. Send a chat message asking to browse recent unread emails
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc020", "message": "Show me my latest unread emails"}`
   - **Expected**: Response status 200

2. Verify the response includes tool_calls_made with list_emails
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: `tool_calls_made` array contains at least one entry with `name: "list_emails"`

3. Verify the list_emails tool was called with appropriate filters
   - **Target**: The `arguments` field of the list_emails tool call
   - **Expected**: `arguments` includes `is_read: false` and/or `sort: "newest"` filter

4. Verify the response lists emails
   - **Target**: Response JSON `message.content`
   - **Expected**: Content describes multiple emails with subjects and/or senders

## Success Criteria
- [ ] Response includes `tool_calls_made` with `list_emails` tool
- [ ] list_emails arguments include relevant filters (is_read and/or sort)
- [ ] Response content lists emails in a readable format

## Failure Criteria
- AI does not use list_emails (uses search_emails without keywords instead)
- Response is empty or generic without email details
- Non-200 response status

## Notes
- list_emails is preferred over search_emails when no keyword search is needed (just browsing/filtering by metadata)
- The AI should use is_read=false filter based on "unread" in the prompt
