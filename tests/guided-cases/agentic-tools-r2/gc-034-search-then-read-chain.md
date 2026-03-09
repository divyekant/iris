# GC-034: Search Then Read Chain — Google Security Alert

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, multi-tool, search_emails, read_email, tool-chaining, google
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains 2 Google security alert emails (Mar 5):
  - Subject: "Security alert" — one about new sign-in, one about new passkey
  - Sender: no-reply@accounts.google.com
- Valid session token available

## Steps

### Step 1: Send a query requiring search followed by read
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Search for the Google security alert about a new sign-in and read me the full content"
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array contains at least 2 tool calls
  - The agentic loop performed multiple iterations

### Step 2: Verify search tool was called first
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` array ordering
- **Expected**:
  - First tool call is `search_emails` or `list_emails` with Google/security-related parameters
  - Search tool returned results including the security alert email

### Step 3: Verify read_email was called with a message ID
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` array for read_email
- **Expected**:
  - `tool_calls_made` includes a `read_email` tool call
  - `read_email` call has a `message_id` parameter (8-char truncated ID or full ID)
  - The message_id corresponds to one of the Google security alert emails

### Step 4: Verify response contains full email content
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text
- **Expected**:
  - Response includes details from the email body (sign-in notification content)
  - Response is more detailed than just subject/sender (includes body text)
  - Citations include the Google security alert email

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` includes both a search/list tool call AND a `read_email` call
- [ ] Search tool was called before read_email (correct chaining order)
- [ ] `read_email` was called with a valid message_id
- [ ] Response contains email body content (not just metadata)
- [ ] Citations include the Google security alert

## Failure Criteria
- API returns non-200 status code
- Only one tool was called (no chaining occurred)
- `read_email` was called without a preceding search (random ID guess)
- `read_email` message_id is invalid or doesn't match a Google email
- Response only contains subject/sender without body content
- Agentic loop exceeded max iterations (5) without completing

## Notes
- This tests the agentic loop's ability to chain multiple tools across iterations. The AI should: (1) search for the email, (2) extract the message_id from search results, (3) call read_email with that ID. This is a core multi-hop capability of the V11 agentic architecture. The 8-char truncated ID format should work with read_email's ID resolution.
