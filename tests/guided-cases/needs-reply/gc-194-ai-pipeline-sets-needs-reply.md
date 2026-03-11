# GC-194: AI Pipeline AiMetadata Includes needs_reply

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, ai-pipeline, classification
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy (Ollama, Anthropic, or OpenAI)

### Data
- A new email synced after the AI pipeline is active
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Trigger a sync or wait for IDLE to pull a new message
   - **Target**: New email arrives in the synced account
   - **Input**: n/a
   - **Expected**: The AI classification job is enqueued and processed

2. Fetch the newly synced message via the message detail endpoint
   - **Target**: `GET /api/messages/{message_id}`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; the message has `ai_needs_reply` set to `true` or `false` (not `null`)

3. Verify the AI classification populated needs_reply
   - **Target**: The `ai_needs_reply` field in the response
   - **Input**: n/a
   - **Expected**: Value is a boolean, confirming the AI pipeline extracted the needs_reply signal

## Success Criteria
- [ ] Newly synced message has `ai_needs_reply` as a boolean (not null)
- [ ] The value reflects whether the email requires a reply (e.g., a question → true, a newsletter → false)
- [ ] No server error during AI processing

## Failure Criteria
- `ai_needs_reply` remains `null` after AI processing
- AI classification job fails with an error
- The field is missing from the response
