# GC-349: Happy Path — Detect Social Engineering in Suspicious Email

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, detect, urgency-pressure, fear-threat, high-risk, ai-analysis
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists with a body containing clear manipulation tactics, e.g.:
  > "URGENT: Your account will be permanently suspended in 24 hours. Click this link immediately to verify your identity or lose all access forever. Failure to act will result in legal action."
- The message body is > 20 characters
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis on the suspicious message
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK with a `SocialEngineeringResult` JSON object

3. Inspect the returned result
   - **Target**: (response body from step 2)
   - **Expected**:
     - `risk_level` is one of `"medium"`, `"high"`, or `"critical"` (not `"none"` or `"low"`)
     - `tactics` is a non-empty array
     - Each tactic has `type`, `evidence`, and `confidence` fields
     - At least one tactic `type` is `"urgency_pressure"` or `"fear_threat"`
     - Each tactic `confidence` is a number between 0.0 and 1.0 (inclusive)
     - `summary` is a non-empty string

## Success Criteria
- [ ] Response status is 200
- [ ] `risk_level` is `"medium"`, `"high"`, or `"critical"`
- [ ] `tactics` array is non-empty
- [ ] Each tactic has `type`, `evidence`, and `confidence` fields
- [ ] All tactic `type` values are from the valid set: `urgency_pressure`, `authority_exploitation`, `fear_threat`, `reward_lure`, `trust_exploitation`, `information_harvesting`
- [ ] All `confidence` values are in range [0.0, 1.0]
- [ ] `summary` is a non-empty string explaining the detected manipulation

## Failure Criteria
- Response status is not 200
- `risk_level` is `"none"` or `"low"` for a clearly threatening message
- `tactics` array is empty despite high risk level
- Any tactic `type` is not from the valid 6-type set
- Any `confidence` value is outside [0.0, 1.0]
- `summary` is absent or empty
- Server returns 500
