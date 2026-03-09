# GC-031: Date Range Filter — March 5th Emails

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, date-filter, date_from, date_to, google
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains 2 Google security alert emails dated March 5, 2026
- Other emails span different dates (StockX various dates, TataUnistore Mar 7, etc.)
- Valid session token available

## Steps

### Step 1: Send a date-filtered query
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Show me emails from March 5th"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call includes date_from and/or date_to parameters
  - Date parameters reference 2026-03-05 (or equivalent date format)

### Step 2: Verify date filter was applied correctly
- **Target**: Response from Step 1
- **Input**: Inspect tool call arguments in `tool_calls_made`
- **Expected**:
  - Tool call arguments contain `date_from` set to "2026-03-05" or similar
  - Tool call arguments contain `date_to` set to "2026-03-05" or "2026-03-06" (end of day boundary)
  - The AI correctly interpreted "March 5th" as a specific date

### Step 3: Verify Google security alerts appear in results
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text and `"citations"` array
- **Expected**:
  - Response references Google security alerts (new sign-in, new passkey)
  - Google emails from Mar 5 are included in the results
  - Any StockX emails returned are also from Mar 5 (if any exist for that date)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes a tool call with date_from and/or date_to filters
- [ ] Date filter values correctly represent March 5, 2026
- [ ] Google security alert emails appear in the results
- [ ] Results are scoped to the requested date (no emails from other dates dominate)

## Failure Criteria
- API returns non-200 status code
- AI answers without using date filters in tool calls
- Date filter is malformed or references wrong date
- Response returns empty results despite known Mar 5 emails
- Response is dominated by emails from other dates with no date filtering visible

## Notes
- Addresses Round 1 weakness #3: no filter-specific testing. This validates date_from/date_to filter parameters. The AI must correctly resolve "March 5th" to an actual date and apply it as a filter. Note: some StockX emails may also fall on Mar 5, which is acceptable.
