# GC-350: Clean Email Returns risk_level "none"

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, detect, clean-email, no-risk, safe-sender, ai-analysis
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists with a benign, professional body, e.g.:
  > "Hi team, just a reminder that our weekly sync is scheduled for Thursday at 2pm. Please add agenda items to the shared doc before then. Thanks!"
- The message body is > 20 characters
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis on the clean message
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK with a `SocialEngineeringResult` JSON object

3. Inspect the returned result
   - **Target**: (response body from step 2)
   - **Expected**:
     - `risk_level` equals `"none"`
     - `tactics` is an empty array `[]`
     - `summary` is a string (may be empty or briefly note no manipulation detected)

## Success Criteria
- [ ] Response status is 200
- [ ] `risk_level` equals `"none"`
- [ ] `tactics` is an empty array or absent
- [ ] No false-positive tactic detections for normal professional language

## Failure Criteria
- Response status is not 200
- `risk_level` is anything other than `"none"` or `"low"` for a clearly benign message
- `tactics` array is non-empty (false positive)
- Server returns 500
