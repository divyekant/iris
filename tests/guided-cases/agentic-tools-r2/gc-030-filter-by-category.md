# GC-030: Filter by Category — Promotional Emails

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, category-filter, list_emails, promotions
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains ~44 promotional emails (predominantly StockX)
- Category breakdown: Primary:3, Promotions:44, Social:6, Updates:2
- Valid session token available

## Steps

### Step 1: Send a category-filtered query
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Show me my promotional emails"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call includes a category filter with value "promotions", "Promotions", or "promotional"

### Step 2: Verify promotional emails dominate results
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text and `"citations"` array
- **Expected**:
  - Response references StockX emails (the dominant sender in Promotions)
  - Response mentions promotional subjects like deals, discounts, product drops
  - Citations (if present) predominantly contain StockX-related subjects

### Step 3: Verify non-promotional emails are excluded
- **Target**: Response from Step 1
- **Input**: Inspect response for cross-category contamination
- **Expected**:
  - Response does NOT include Google security alerts (Primary category)
  - Response does NOT include "Test from Iris" emails (Updates category)
  - Response does NOT include "Re: Welcome to email bro" (Social category)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes a tool call with category filter for promotions
- [ ] Response content is dominated by StockX / promotional email references
- [ ] No non-promotional category emails appear in results
- [ ] Response acknowledges the volume (~44 promotional emails)

## Failure Criteria
- API returns non-200 status code
- AI answers without using category filter in tool calls
- Response includes emails from Primary, Social, or Updates categories
- Response returns empty results despite 44 promotional emails existing
- Category filter value is malformed or not recognized

## Notes
- Addresses Round 1 weakness #3: no filter-specific testing. This validates the category filter parameter specifically. The AI may use list_emails (preferred for browsing) or search_emails (if it interprets "promotional" as a search term).
