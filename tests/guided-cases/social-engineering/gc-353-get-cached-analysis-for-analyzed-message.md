# GC-353: GET Cached Analysis for Analyzed Message

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, get-cached, read-endpoint, after-analysis
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists with body > 20 characters
- The message ID is known as `{msg_id}`
- The message has already been analyzed via POST /api/ai/detect-social-engineering (cached result exists)

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Ensure analysis has been run (setup)
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK; record the response body as `{post_result}`

3. Fetch the cached result via the GET endpoint
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}/social-engineering
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a `SocialEngineeringResult` JSON body matching `{post_result}`

4. Verify field consistency
   - **Target**: (compare GET response body with `{post_result}`)
   - **Expected**: `risk_level`, `tactics`, and `summary` are identical

## Success Criteria
- [ ] GET request returns 200
- [ ] GET response `risk_level` matches the POST response `risk_level`
- [ ] GET response `tactics` matches the POST response `tactics`
- [ ] GET response `summary` matches the POST response `summary`

## Failure Criteria
- GET request returns non-200 status (including 404)
- Any field differs between GET and POST responses
- GET response is missing required fields (`risk_level`, `tactics`, `summary`)
- Server returns 500
