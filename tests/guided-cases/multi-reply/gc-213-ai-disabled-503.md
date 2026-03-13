# GC-213: AI disabled returns 503 Service Unavailable

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: ai-disabled, service-unavailable, provider
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI disabled: config table has `ai_enabled = "false"` (or no AI providers configured)

### Data
- Existing thread with messages (source: synced inbox)
- Known `thread_id` for that thread

## Steps
1. Ensure AI is disabled via settings
   - **Target**: `PUT /api/settings` or direct DB config
   - **Input**: Set `ai_enabled` to `"false"`
   - **Expected**: AI is disabled for all endpoints

2. Send multi-reply request
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>" }`
   - **Expected**: 503 Service Unavailable

3. Re-enable AI (cleanup)
   - **Target**: `PUT /api/settings` or direct DB config
   - **Input**: Set `ai_enabled` to `"true"`
   - **Expected**: AI re-enabled

## Success Criteria
- [ ] Response status is 503 when AI is disabled
- [ ] No AI generation is attempted
- [ ] Endpoint works normally after re-enabling AI

## Failure Criteria
- Status other than 503
- Server returns 500 or attempts to call a nonexistent provider
- AI state change does not take effect

## Notes
The handler checks both `ai_enabled != "true"` and `!state.providers.has_providers()`. Either condition triggers 503. This case covers the config-based disable path.
