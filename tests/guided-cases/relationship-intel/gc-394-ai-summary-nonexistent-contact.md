# GC-394: Negative — AI summary for non-existent contact returns error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, ai, not-found, error-handling
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and healthy

### Data
- An email address that has never appeared in any synced message (e.g., `nobody@does-not-exist.test`)

## Steps
1. Request AI summary for a non-existent contact
   - **Target**: `POST /api/contacts/nobody@does-not-exist.test/intelligence/ai-summary`
   - **Input**: Header `X-Session-Token: {token}`, body `{}`
   - **Expected**: 404 Not Found with descriptive error message OR 200 with a summary that explicitly states no data is available

2. Verify no hallucinated data
   - **Target**: Response body (if 200)
   - **Input**: Read `summary` and `key_insights`
   - **Expected**: Summary does not contain fabricated statistics or relationship details about the contact — must acknowledge lack of data rather than invent facts

3. Verify error response format (if 404)
   - **Target**: Response JSON
   - **Input**: Parse error object
   - **Expected**: `error` or `message` field present; no internal AI prompt or stack trace exposed

## Success Criteria
- [ ] Response is 404 or 200 (no 500)
- [ ] If 200: summary explicitly states no message history exists; no fabricated stats
- [ ] If 404: error message is present and informative
- [ ] No AI provider error details leaked to the client
- [ ] No hallucinated relationship data presented as fact

## Failure Criteria
- 500 Internal Server Error
- AI provider error message exposed verbatim (e.g., Anthropic/Ollama error JSON)
- Summary invents relationship history for an address with zero messages
- Prompt template exposed in the response body

## Notes
The AI summary endpoint must gracefully handle the zero-data case. The AI model should not hallucinate — if no messages exist, the response should say so rather than fabricate a relationship narrative. A 404 short-circuit before calling the AI provider is the preferred implementation path.
