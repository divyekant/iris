# GC-033: Compound Filter — Sender + Category + Read Status

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, compound-filter, sender, category, is_read, search_emails, list_emails
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains ~44 StockX promotional emails, most unread
- Category: Promotions for StockX emails
- Valid session token available

## Steps

### Step 1: Send a query with multiple filter constraints
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Find all unread emails from StockX in the promotions category"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call includes multiple filters simultaneously

### Step 2: Verify compound filters in tool call arguments
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` for filter parameters
- **Expected**:
  - Tool call arguments include sender filter containing "StockX" or "stockx"
  - Tool call arguments include category filter for "promotions" or "Promotions"
  - Tool call arguments include is_read filter set to false
  - All three filters applied in a single tool call (not separate calls)

### Step 3: Verify results match all filter criteria
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text and `"citations"` array
- **Expected**:
  - Response references only StockX emails
  - Response confirms these are unread promotional emails
  - No emails from other senders or categories appear
  - Result count is plausible (~40+ unread StockX promotions)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes tool call with sender filter for StockX
- [ ] `tool_calls_made` includes tool call with category filter for promotions
- [ ] `tool_calls_made` includes tool call with is_read:false filter
- [ ] At least 2 of the 3 filters appear in the same tool call
- [ ] Response content is exclusively StockX promotional emails
- [ ] No errors from compound filter application

## Failure Criteria
- API returns non-200 status code
- AI uses only one filter and ignores the others
- Tool call has no filters (AI answered from system prompt alone)
- Response includes non-StockX or non-promotional emails
- SQL error or tool execution failure from combining filters
- AI makes separate single-filter calls instead of combining them

## Notes
- Addresses Round 1 weakness #3 (no filter-specific testing) and #5 (no direct tool handler testing). This tests the tool's ability to handle multiple simultaneous filter parameters. The key edge case is whether the backend correctly ANDs the filters together. If the AI splits into multiple tool calls, that's suboptimal but still acceptable as long as the final answer is correct.
