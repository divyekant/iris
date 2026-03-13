# GC-221: Missing Both IDs — 400 Bad Request

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: validation, missing-input, 400, error
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- None required (source: inline)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Send extract-tasks request with neither message_id nor thread_id
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{}`
   - **Expected**: 400 Bad Request

3. Send extract-tasks request with both fields explicitly null
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"message_id": null, "thread_id": null}`
   - **Expected**: 400 Bad Request

## Success Criteria
- [ ] Step 2 returns 400 Bad Request
- [ ] Step 3 returns 400 Bad Request
- [ ] No AI call is made (no 502 or 503 error)
- [ ] Response does not leak internal details

## Failure Criteria
- Either request returns 200, 500, or any status other than 400
- Server panics or crashes on empty input
- AI provider is invoked despite missing input

## Notes
The handler checks `input.message_id.is_none() && input.thread_id.is_none()` before any database or AI calls. Both empty-body and explicit-null cases should hit this guard.
