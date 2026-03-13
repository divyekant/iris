# GC-358: risk_level > low with No Tactics Downgraded to "none"

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, validation, downgrade, risk-level, no-tactics, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured — this case relies on the server's post-processing logic that enforces the invariant: if no tactics are present, risk_level must not exceed "low"

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists that, when analyzed, causes the AI to return (or can be simulated to return) a response with `risk_level: "high"` but an empty `tactics` array — an inconsistent AI output
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis on the target message
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK

3. Inspect the returned `risk_level` and `tactics`
   - **Target**: (response body from step 2)
   - **Expected**:
     - `tactics` is an empty array `[]`
     - `risk_level` equals `"none"` (downgraded from the AI's spurious `"high"`)
     - The inconsistent AI output was corrected by server-side validation before being returned or cached

## Notes
If a live AI call cannot be reliably coerced to produce this inconsistency, this case may be verified by:
1. Examining the server validation code that enforces the downgrade rule
2. Writing a unit test that passes `{ risk_level: "high", tactics: [] }` through the validation function and asserts the output is `"none"`
In either scenario, document the verification method in the execution report.

## Success Criteria
- [ ] Response status is 200
- [ ] `tactics` is an empty array
- [ ] `risk_level` equals `"none"` (not `"high"`, `"medium"`, or `"critical"`)
- [ ] The downgrade rule is applied consistently (verified via code path or unit test)

## Failure Criteria
- Response status is not 200
- `risk_level` is `"high"` (or any value > `"low"`) when `tactics` is empty (invariant violated)
- `tactics` contains phantom entries that were not in the original AI response
- Server returns 500
