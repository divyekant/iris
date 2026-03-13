# GC-215: Multi-reply without context omits guidance from prompt

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: no-context, default-behavior, optional-field
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and enabled

### Data
- Existing thread with at least one message (source: synced inbox)
- Known `thread_id` for that thread

## Steps
1. Send multi-reply request without context field
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>" }`
   - **Expected**: 200 OK with 3 reply options

2. Send multi-reply request with context explicitly null
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>", "context": null }`
   - **Expected**: 200 OK with 3 reply options

3. Compare response structure
   - **Target**: Both responses
   - **Input**: Validate both responses
   - **Expected**: Both return valid 3-option arrays with formal/casual/brief tones

## Success Criteria
- [ ] Both requests return status 200
- [ ] Both responses have `options` arrays with exactly 3 elements
- [ ] All options have valid tone, subject, and body fields
- [ ] Omitting context does not cause an error

## Failure Criteria
- Missing context field causes deserialization error or 400
- Null context causes a different error than omitted context
- Fewer than 3 options returned

## Notes
The `context` field is `Option<String>` in `MultiReplyRequest`, so both omission and explicit null are valid. The `build_multi_reply_prompt` function only appends the guidance line when context is `Some(...)`.
