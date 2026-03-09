# GC-021: AI Uses Read Email for Detail Request

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, read_email, tool-use, detail
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- At least one email with substantive body text exists in the database (source: local-db)

## Steps

1. First, search for an email to get an ID
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc021", "message": "Find me an email and tell me the full details of the most recent one"}`
   - **Expected**: Response status 200

2. Verify the response includes read_email in tool_calls_made
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: `tool_calls_made` array contains at least one entry with `name: "read_email"` (the AI should search first, then read the full email)

3. Verify the response includes detailed email content
   - **Target**: Response JSON `message.content`
   - **Expected**: Content includes email body details, not just subject/sender summary

## Success Criteria
- [ ] Response includes `tool_calls_made` with `read_email` tool
- [ ] AI performed multi-step retrieval (search/list then read)
- [ ] Response content includes email body details

## Failure Criteria
- AI never uses read_email tool
- Response only contains summary-level info (no body content)
- Non-200 response status

## Notes
- read_email returns the full email body (truncated at 4000 chars for safety)
- The AI typically chains: search_emails or list_emails first to find IDs, then read_email for full content
