# GC-356: Invalid Tactic Types Filtered Out

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, validation, tactic-type, filter, malformed-ai-response, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured — this case relies on the server's validation layer intercepting an AI response that includes unrecognized tactic type strings

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists that, when analyzed, causes the AI to return (or can be simulated to return) a tactic with a type outside the valid set, e.g. `"emotional_manipulation"` or `"bribery"`
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK

3. Inspect the `tactics` array in the response
   - **Target**: (response body from step 2)
   - **Expected**:
     - Every tactic `type` in the returned array is one of the 6 valid values: `urgency_pressure`, `authority_exploitation`, `fear_threat`, `reward_lure`, `trust_exploitation`, `information_harvesting`
     - No tactic with an unrecognized type appears in the response

## Notes
If the AI provider cannot be made to return an invalid type in a live run, this case may be verified by examining the server-side deserialization/validation code and confirming it would reject unknown enum variants, then marking as "verified by code inspection."

## Success Criteria
- [ ] Response status is 200
- [ ] All `tactics[*].type` values are from the valid 6-type set
- [ ] No invalid tactic type leaks into the response
- [ ] `summary` and `risk_level` are still present and valid

## Failure Criteria
- Response status is not 200
- Any tactic `type` is outside the valid set (e.g. `"emotional_manipulation"`, `"bribery"`, `"phishing"`)
- Server returns 500 when AI returns an unexpected type rather than filtering gracefully
