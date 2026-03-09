# GC-036: Very Long Query Handling

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, edge-case, long-input, stress, input-validation
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Standard inbox with ~55 emails
- API input cap is 50,000 characters
- Valid session token available

## Steps

### Step 1: Send a very long query (200+ words)
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "I need your help analyzing my email inbox. I want to understand several things about my email patterns and communication habits. First, I would like to know who sends me the most emails and what those emails are generally about. Second, I want to understand the distribution of my emails across different categories like primary, promotions, social, and updates. Third, I need to know how many of my emails are currently unread and whether there are any important unread emails I should pay attention to right away. Fourth, can you tell me about any security-related emails I may have received, particularly from Google or other service providers that might require my attention. Fifth, I want to know if there are any emails about deals, discounts, or special offers that might be expiring soon. Sixth, please check if there are any delivery failure notifications that I should follow up on. Seventh, I would appreciate a summary of my most recent emails from the past few days. Eighth, can you identify any patterns in when I receive the most emails during the day or week. Ninth, are there any emails that appear to be from personal contacts versus automated or marketing senders. Finally, tenth, please provide an overall health assessment of my inbox including whether I should archive or delete any categories of emails to keep things organized."
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response (not timeout, not 413, not 500)
  - Response is generated within a reasonable time (under 60 seconds)
  - API does not crash or hang

### Step 2: Verify the response is coherent
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text
- **Expected**:
  - Response is a non-empty string
  - Response addresses at least some of the 10 questions asked
  - Response is coherent and not truncated mid-sentence
  - AI prioritized the most answerable questions given available tools

### Step 3: Verify tool usage was appropriate
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` array
- **Expected**:
  - `tool_calls_made` is present (AI used tools to gather information)
  - Tool calls are reasonable (inbox_stats, search_emails, list_emails)
  - Agentic loop did not exceed max iterations (5)
  - No error entries in tool_calls_made

## Success Criteria
- [ ] HTTP 200 returned (no timeout or server error)
- [ ] Response generated within 60 seconds
- [ ] Response is non-empty and coherent
- [ ] Response addresses multiple parts of the multi-part query
- [ ] `tool_calls_made` shows appropriate tool usage
- [ ] Agentic loop stayed within max iterations (5)
- [ ] No crashes, hangs, or memory issues

## Failure Criteria
- API returns timeout error (408 or similar)
- API returns 413 Payload Too Large
- API returns 500 Internal Server Error
- Response is empty or contains only an error message
- Server hangs indefinitely processing the long query
- Agentic loop enters infinite iteration attempting to answer all 10 questions
- Response is incoherent or severely truncated

## Notes
- Addresses Round 1 weakness #4: no adversarial/edge cases with very long queries. The 200+ word message tests: (1) input handling for large payloads, (2) the AI's ability to prioritize when given more questions than the 5-iteration limit can handle, (3) overall system stability under non-trivial input sizes. The message is ~1,500 characters, well within the 50,000 char cap but large enough to stress-test prompt construction. The AI is expected to use multiple tools but cannot address all 10 questions in 5 iterations — graceful prioritization is the key behavior.
