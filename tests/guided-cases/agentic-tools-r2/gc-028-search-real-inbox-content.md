# GC-028: Search with Real Inbox Content

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, search_emails, citations, stockx
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains ~44 StockX promotional emails with shoe-related subjects (e.g., "New AJ4 Imperial Purple", "AJ5 Wolf Grey", "Converse Chaos")
- Valid session token available

## Steps

### Step 1: Send a search query about real inbox content
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Search for emails from StockX about shoes"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - Response body contains `"response"` field with a non-empty string
  - `tool_calls_made` array is present and non-empty
  - At least one tool call has `"tool"` equal to `"search_emails"` or `"list_emails"`

### Step 2: Verify search results are non-empty
- **Target**: Response from Step 1
- **Input**: Inspect the `"response"` text
- **Expected**:
  - Response text references StockX or specific shoe subjects (AJ4, AJ5, Converse, etc.)
  - Response does not say "no results found" or "I couldn't find any"

### Step 3: Verify citations are populated
- **Target**: Response from Step 1
- **Input**: Inspect `"citations"` array in response body
- **Expected**:
  - `citations` array is present and contains at least 1 entry
  - Each citation has `message_id` and `subject` fields
  - At least one citation subject contains a StockX-related keyword

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes `search_emails` or `list_emails`
- [ ] Response text references StockX emails with real subjects
- [ ] `citations` array is non-empty with valid message_id and subject fields
- [ ] No error messages or empty-result indicators in the response

## Failure Criteria
- API returns non-200 status code
- `tool_calls_made` is empty (AI answered without using tools)
- Response says no emails found despite ~44 StockX emails existing
- Citations array is empty or missing
- Response references emails not present in the inbox

## Notes
- This case directly addresses Round 1 weakness #1: previous cases used queries like "budget" and "meetings" that returned empty results. StockX emails are the dominant content in the test inbox.
