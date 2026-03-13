# GC-352: Cached Result Returned on Repeat Call

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, caching, idempotent, repeat-call, social_engineering_analysis
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists with body > 20 characters (any content suffices)
- The message ID is known as `{msg_id}`
- The message has NOT yet been analyzed (no cached result in `social_engineering_analysis` table)

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis for the first time
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK; record the full response body as `{first_result}`

3. Trigger social engineering analysis a second time for the same message
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK; record the full response body as `{second_result}`

4. Compare the two results
   - **Target**: (compare `{first_result}` and `{second_result}`)
   - **Expected**: `risk_level`, `tactics`, and `summary` fields are identical between the two responses

## Success Criteria
- [ ] Both POST requests return 200
- [ ] `{first_result}.risk_level` equals `{second_result}.risk_level`
- [ ] `{first_result}.tactics` equals `{second_result}.tactics` (same array contents)
- [ ] `{first_result}.summary` equals `{second_result}.summary`
- [ ] Second response is noticeably faster than first (cache hit, no AI round-trip)

## Failure Criteria
- Either request returns non-200 status
- `risk_level`, `tactics`, or `summary` differ between calls (non-deterministic, uncached behavior)
- Second call takes as long as first (cache miss — result not persisted)
- Server returns 500
