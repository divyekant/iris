# GC-354: GET Cached for Non-Analyzed Message Returns 404

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, get-cached, 404, not-yet-analyzed, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists that has NOT yet been analyzed via POST /api/ai/detect-social-engineering
- The message ID is known as `{msg_id}` and confirmed to have no cached result in `social_engineering_analysis`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request the cached analysis for the unanalyzed message
   - **Target**: GET http://127.0.0.1:3000/api/messages/{msg_id}/social-engineering
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 404 Not Found

3. Inspect the response body
   - **Target**: (response body from step 2)
   - **Expected**: Response body contains an error or message indicating no cached analysis exists (e.g. `{ "error": "not found" }` or similar); body is not a `SocialEngineeringResult` object

## Success Criteria
- [ ] Response status is 404
- [ ] Response body does not contain a `SocialEngineeringResult` structure
- [ ] No server error (not 500)

## Failure Criteria
- Response status is 200 (should not invent a result)
- Response status is 500 (server error instead of clean 404)
- Response body contains a fabricated `SocialEngineeringResult` object
