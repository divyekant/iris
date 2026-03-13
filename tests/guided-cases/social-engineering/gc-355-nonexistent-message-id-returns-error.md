# GC-355: Non-Existent message_id Returns Error

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, invalid-input, 404, unknown-message, negative
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A message ID that does not exist in the database, e.g. `"msg-does-not-exist-999999"`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger analysis with a non-existent message ID
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "msg-does-not-exist-999999" }`
   - **Expected**: 404 Not Found (or 400 Bad Request); response body contains an error description

3. Verify no crash or partial result is returned
   - **Target**: (response body from step 2)
   - **Expected**: Response body is an error object, not a `SocialEngineeringResult` structure

## Success Criteria
- [ ] Response status is 404 or 400
- [ ] Response body contains an error message (not a `SocialEngineeringResult`)
- [ ] Server does not crash (no 500)

## Failure Criteria
- Response status is 200 with a fabricated result
- Response status is 500 (unhandled DB miss)
- Server crashes or returns an empty body
