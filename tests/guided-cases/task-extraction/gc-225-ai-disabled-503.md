# GC-225: AI Disabled — 503 Service Unavailable

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: ai-disabled, 503, service-unavailable, error
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI is **disabled**: either `ai_enabled` config set to `"false"` or no AI providers configured

### Data
- Any valid thread_id from the database (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Disable AI (if not already disabled)
   - **Target**: `PUT http://localhost:3030/api/settings`
   - **Input**: Header `X-Session-Token: {token}`, Body with AI disabled configuration
   - **Expected**: AI features become unavailable

3. Attempt to extract tasks with AI disabled
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{valid_thread_id}"}`
   - **Expected**: 503 Service Unavailable

4. Re-enable AI (cleanup)
   - **Target**: `PUT http://localhost:3030/api/settings`
   - **Input**: Header `X-Session-Token: {token}`, Body with AI enabled configuration
   - **Expected**: AI features become available again

## Success Criteria
- [ ] Step 3 returns 503 Service Unavailable
- [ ] No AI provider call is attempted
- [ ] The check happens before message lookup (efficient early return)
- [ ] Error does not leak provider configuration details

## Failure Criteria
- Request returns 200 with empty tasks instead of 503
- Request returns 500 or a different error code
- Server attempts to call AI provider despite being disabled

## Notes
The handler checks `ai_enabled != "true" || !state.providers.has_providers()` before fetching messages. Both conditions (config flag off, no providers) should trigger 503. Note: the AI-enabled check actually happens *after* the `message_id`/`thread_id` validation but *before* message fetching in the handler — the check order is: (1) neither ID → 400, (2) AI disabled → 503, (3) message lookup.
