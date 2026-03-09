# GC-035: Special Characters in Query — Percent Sign

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, edge-case, special-chars, sql-safety, search_emails
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains StockX email with subject containing "70% Off"
- The "%" character is a SQL LIKE metacharacter and could cause issues if not escaped
- Valid session token available

## Steps

### Step 1: Send a query containing special characters
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Search for emails with subject containing 70% Off"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response (not 500 or error)
  - No SQL injection or LIKE metacharacter errors
  - `tool_calls_made` array is present and non-empty

### Step 2: Verify no SQL errors in response
- **Target**: Response from Step 1
- **Input**: Inspect full response body
- **Expected**:
  - Response does NOT contain "SQL error", "syntax error", "LIKE", or database error messages
  - Response does NOT contain a 500-level error or stack trace
  - The search executed successfully (even if results vary)

### Step 3: Verify search results reference the target email
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text
- **Expected**:
  - Response references the "70% Off" StockX email or similar discount emails
  - If FTS5 search is used, the "%" may be treated as a literal character
  - Response provides meaningful results (not empty due to escaping issues)

## Success Criteria
- [ ] HTTP 200 returned (no server error)
- [ ] No SQL error messages in the response body
- [ ] `tool_calls_made` includes `search_emails` or `list_emails`
- [ ] The "%" character did not cause a crash, SQL injection, or malformed query
- [ ] Response references discount/sale-related StockX emails
- [ ] API handled the special character gracefully

## Failure Criteria
- API returns 500 or other error status code
- Response contains SQL error messages or stack traces
- The "%" character causes LIKE pattern matching to return unexpected results
- Server crashes or hangs due to unescaped special character
- Response is completely empty or nonsensical due to query parsing failure

## Notes
- Addresses Round 1 weakness #4: no adversarial/edge cases. The "%" is a LIKE wildcard in SQL. If the search query is passed directly into a LIKE clause without escaping, "70% Off" becomes "70[any chars] Off" which could return unexpected results or errors. FTS5 MATCH queries handle "%" differently than LIKE. This test verifies the input sanitization path. Other special characters worth testing in future rounds: single quotes, semicolons, backslashes.
