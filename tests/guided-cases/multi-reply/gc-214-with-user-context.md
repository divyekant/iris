# GC-214: Multi-reply with user context influences generated options

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: context, prompt-injection, user-guidance
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
1. Send multi-reply request with context guidance
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>", "context": "Decline the invitation politely, mention a scheduling conflict" }`
   - **Expected**: 200 OK with 3 reply options

2. Verify context influenced the generated replies
   - **Target**: Response JSON
   - **Input**: Inspect `body` of each option
   - **Expected**: At least one option's body references declining, a scheduling conflict, or similar decline language

3. Verify structure remains correct
   - **Target**: Response JSON
   - **Input**: Check all 3 options
   - **Expected**: Still exactly 3 options with formal/casual/brief tones, each with subject and body

## Success Criteria
- [ ] Response status is 200
- [ ] `options` array has exactly 3 elements
- [ ] Generated bodies reflect the context guidance (decline language present)
- [ ] All three tones (formal, casual, brief) are represented
- [ ] Each option has non-empty subject and body

## Failure Criteria
- Context is ignored (replies are generic, unrelated to declining)
- Fewer or more than 3 options
- Missing tone coverage

## Notes
The `build_multi_reply_prompt` function appends the context as "User's guidance: {context}" to the AI prompt. This case verifies that user-provided context steers the AI generation.
