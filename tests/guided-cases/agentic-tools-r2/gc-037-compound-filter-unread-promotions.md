# GC-037: Compound Filter — Unread Promotions Count and List

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, compound-filter, is_read, category, list_emails, count
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains ~44 promotional emails (predominantly StockX), most unread
- Expected ~40+ unread promotional emails
- Category breakdown: Promotions:44
- Valid session token available

## Steps

### Step 1: Send a compound filter query requesting count and list
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "How many unread promotional emails do I have? List the 5 most recent."
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call includes both is_read and category filters

### Step 2: Verify compound filters in tool call
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` for filter parameters
- **Expected**:
  - Tool call includes `is_read` set to `false` (for unread)
  - Tool call includes `category` matching "promotions" or "Promotions"
  - Tool call may include `limit` set to 5 or `sort` for recency
  - At least `is_read` and `category` appear together in one tool call

### Step 3: Verify count in response
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text for count
- **Expected**:
  - Response provides a specific count of unread promotional emails
  - Count is approximately 40+ (given ~44 promotions, most unread)
  - Count is plausible (not 0, not 1000)

### Step 4: Verify 5 most recent are listed
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text for email listing
- **Expected**:
  - Response lists approximately 5 specific emails with subjects
  - Listed emails are StockX promotional emails (dominant in promotions category)
  - Emails appear to be ordered by recency (most recent first)
  - Subjects match known StockX emails (AJ4, AJ5, Converse, Savings Spotlight, etc.)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes tool call with `is_read:false` filter
- [ ] `tool_calls_made` includes tool call with category filter for promotions
- [ ] Response provides a specific unread promotional email count (~40+)
- [ ] Response lists approximately 5 specific recent promotional emails
- [ ] Listed emails are real StockX emails from the inbox
- [ ] Both the count and list aspects of the query are addressed

## Failure Criteria
- API returns non-200 status code
- AI answers only the count OR only the list (not both)
- Filters are not combined (separate calls for is_read and category without intersection)
- Count is wildly inaccurate (e.g., 0 or 100+)
- Listed emails are not from the promotions category
- Response ignores the "5 most recent" constraint entirely

## Notes
- Addresses Round 1 weakness #3 (no filter-specific testing) with a compound filter combining read status and category. This also tests the AI's ability to handle a two-part question (count + list) which may require the AI to make one tool call with a limit for the list and interpret the total count from tool results. The AI may use inbox_stats for the count and list_emails for the listing, or combine both in a single list_emails call.
