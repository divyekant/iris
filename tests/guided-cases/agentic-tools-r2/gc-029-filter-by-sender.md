# GC-029: Filter by Sender — Google Emails

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, sender-filter, list_emails, search_emails, google
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains 2 Google security alert emails from no-reply@accounts.google.com (dated Mar 5)
- Subjects: "Security alert" (new sign-in notification, new passkey notification)
- Valid session token available

## Steps

### Step 1: Send a sender-filtered query
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Show me all emails from Google"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call uses a sender filter containing "google", "Google", or "no-reply@accounts.google.com"

### Step 2: Verify only Google emails are returned
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text and `"citations"` array
- **Expected**:
  - Response references security alerts or Google account notifications
  - Response does NOT reference StockX, TataUnistore, or Mail Delivery Subsystem emails
  - Citations (if present) have subjects matching "Security alert"

### Step 3: Verify result count is accurate
- **Target**: Response from Step 1
- **Input**: Inspect response text for count or listing
- **Expected**:
  - Response identifies exactly 2 Google emails (or says "a couple" / "two")
  - Response mentions security-related content (sign-in, passkey)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes a tool call with sender filter for Google
- [ ] Response references Google security alerts specifically
- [ ] Response does not include non-Google emails
- [ ] Result count aligns with expected 2 Google emails

## Failure Criteria
- API returns non-200 status code
- AI answers without using any tools (no sender filter applied)
- Response includes StockX or other non-Google emails
- Response says no Google emails found despite 2 existing
- Sender filter is missing from tool call arguments

## Notes
- Addresses Round 1 weakness #3: no filter-specific testing. This case specifically validates the sender filter parameter is passed to the tool and produces correctly scoped results.
