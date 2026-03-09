# GC-032: Force inbox_stats Tool Call — Top Senders Detail

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools-r2
- **Tags**: v11, agentic, inbox_stats, top-senders, tool-call-verification
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- AI provider configured and enabled
- At least one email account synced with messages

### Data
- Inbox contains ~55 emails from multiple senders:
  - StockX: ~44 emails (dominant sender)
  - Google (no-reply@accounts.google.com): 2 emails
  - TataUnistore1: 3 emails
  - Mail Delivery Subsystem: 1 email
- System prompt pre-embeds basic inbox_stats summary
- Valid session token available

## Steps

### Step 1: Send a query requiring detailed sender breakdown
- **Target**: POST http://localhost:3000/api/ai/chat
- **Input**:
  ```json
  {
    "message": "Who are my top senders and how many emails has each sent? Give me the full breakdown."
  }
  ```
  Headers: `X-Session-Token: <valid_token>`, `Content-Type: application/json`
- **Expected**:
  - HTTP 200 response
  - `tool_calls_made` array is present and non-empty
  - At least one tool call has `"tool"` equal to `"inbox_stats"`

### Step 2: Verify inbox_stats tool was explicitly called
- **Target**: Response from Step 1
- **Input**: Inspect `tool_calls_made` array
- **Expected**:
  - `tool_calls_made` contains an entry with `"tool": "inbox_stats"`
  - The AI did NOT just rely on the pre-embedded system prompt stats
  - This confirms the tool was actually invoked at runtime

### Step 3: Verify response contains detailed sender breakdown
- **Target**: Response from Step 1
- **Input**: Inspect `"response"` text
- **Expected**:
  - Response lists individual senders with email counts
  - StockX identified as top sender with ~44 emails
  - Google, TataUnistore, and/or Mail Delivery Subsystem also mentioned
  - Counts are approximately correct (exact numbers may vary slightly)

## Success Criteria
- [ ] HTTP 200 returned
- [ ] `tool_calls_made` explicitly includes `"inbox_stats"` tool call
- [ ] Response provides per-sender email counts (not just a summary)
- [ ] StockX identified as the dominant sender
- [ ] Multiple senders listed with individual counts
- [ ] AI did not just paraphrase the system prompt — it called the tool

## Failure Criteria
- API returns non-200 status code
- `tool_calls_made` does NOT contain `inbox_stats` (AI used only system prompt)
- Response provides only vague summary without per-sender counts
- Sender counts are wildly inaccurate (off by more than 5)
- Response fails to identify StockX as the top sender

## Notes
- Directly addresses Round 1 weakness #2: GC-019 didn't trigger the inbox_stats tool because the system prompt already contained enough info. This query asks for "full breakdown" to push the AI beyond the pre-embedded summary. The key assertion is that `inbox_stats` appears in `tool_calls_made`. If the system prompt already contains top_senders with counts, the AI may still skip the tool call — in that case, the test reveals that the system prompt is too detailed and should be trimmed.
