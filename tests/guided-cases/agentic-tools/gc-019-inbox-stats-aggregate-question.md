# GC-019: AI Uses Inbox Stats for Aggregate Question

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, inbox_stats, tool-use, aggregate
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Messages exist in the database with varying read/unread status (source: local-db)

## Steps

1. Send a chat message asking an aggregate inbox question
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc019", "message": "How many unread emails do I have?"}`
   - **Expected**: Response status 200

2. Verify the response includes tool_calls_made with inbox_stats
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: `tool_calls_made` array contains at least one entry with `name: "inbox_stats"`

3. Verify the response answers the aggregate question
   - **Target**: Response JSON `message.content`
   - **Expected**: Content includes a number or count referencing unread emails

## Success Criteria
- [ ] Response includes `tool_calls_made` with `inbox_stats` tool
- [ ] Response content answers the aggregate question with specific numbers
- [ ] No error in response

## Failure Criteria
- Response returns non-200 status
- AI does not use inbox_stats tool (uses search instead, which is less appropriate)
- Response contains no numerical answer

## Notes
- inbox_stats returns total, unread, starred counts plus category breakdown and top senders
- The AI should prefer inbox_stats over search_emails for "how many" type questions
