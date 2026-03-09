# GC-018: AI Uses Search Tool for Keyword Query

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, search_emails, tool-use
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled (Anthropic/OpenAI/Ollama)
- At least one email account synced with messages in the database

### Data
- Emails exist in the database with searchable content (source: local-db)

## Steps

1. Send a chat message asking about a specific topic present in the inbox
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc018", "message": "Do I have any emails about budget?"}`
   - **Expected**: Response status 200, response body contains `message` object

2. Verify the response includes tool_calls_made with search_emails
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: `tool_calls_made` array is present and non-empty; at least one entry has `name: "search_emails"`

3. Verify the search tool was called with a relevant query
   - **Target**: The `arguments` field of the search_emails tool call
   - **Expected**: `arguments.query` contains a search term related to "budget" (the AI extracts keywords from the user message)

4. Verify the response content is a natural language answer
   - **Target**: Response JSON `message.content`
   - **Expected**: Non-empty string containing a coherent answer about the user's emails (not raw JSON or error text)

## Success Criteria
- [ ] Response includes `tool_calls_made` array
- [ ] At least one tool call has `name: "search_emails"`
- [ ] The search query in arguments is relevant to the user question
- [ ] The response content is a natural language answer

## Failure Criteria
- Response returns non-200 status code
- `tool_calls_made` is null or empty (no tools used)
- Response content is empty or contains raw JSON/error text

## Notes
- The AI model decides which tools to use — the specific query terms may vary
- The response may also include citations if matching emails were found
