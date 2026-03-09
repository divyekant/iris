# GC-022: AI Chains Multiple Tools in One Response

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, multi-tool, chaining, tool-use
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Multiple emails from different senders exist in the database (source: local-db)

## Steps

1. Send a complex question requiring multiple tool calls
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc022", "message": "Give me an overview: how many total emails do I have, and show me the 3 most recent unread ones"}`
   - **Expected**: Response status 200

2. Verify multiple tools were used
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: `tool_calls_made` array has 2 or more entries with different tool names (e.g., inbox_stats + list_emails, or search_emails + list_emails)

3. Verify the response synthesizes information from multiple sources
   - **Target**: Response JSON `message.content`
   - **Expected**: Content answers both parts of the question — aggregate stats AND specific email listings

## Success Criteria
- [ ] `tool_calls_made` contains 2+ tool calls
- [ ] Multiple different tools were used (not just the same tool twice)
- [ ] Response answers both parts of the compound question

## Failure Criteria
- Only a single tool was used
- Response only addresses one part of the question
- Non-200 response status

## Notes
- The agentic loop supports up to 5 iterations, allowing the AI to call tools multiple times
- This tests the multi-turn tool-use capability that V11 introduced
