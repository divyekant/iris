# GC-217: AI returns empty options array — 502 Bad Gateway

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: empty-array, parse-failure, bad-gateway, ai-edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and enabled
- AI model may occasionally return `[]` or refuse to generate options

### Data
- Existing thread with at least one message (source: synced inbox)
- Known `thread_id` for that thread

## Steps
1. Understand the parser behavior for empty arrays
   - **Target**: `parse_multi_reply_response` logic
   - **Input**: Raw AI response: `"[]"`
   - **Expected**: Parser returns `None` because it checks `!options.is_empty()` after deserialization

2. Verify API returns 502 when parser returns None
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>" }` (when AI happens to return `[]`)
   - **Expected**: 502 Bad Gateway — `parse_multi_reply_response` returns `None`, mapped to `StatusCode::BAD_GATEWAY`

## Success Criteria
- [ ] When AI returns an empty JSON array, the API returns 502 (not 200 with empty options)
- [ ] The unit test `test_parse_multi_reply_response_empty_array` confirms `parse_multi_reply_response("[]")` returns `None`

## Failure Criteria
- API returns 200 with `{ "options": [] }` (empty array leaked through)
- API returns 500 instead of 502
- Parser panics on empty array input

## Notes
This is difficult to trigger in a live test because the AI model usually generates content. It can be verified via the unit test `test_parse_multi_reply_response_empty_array` which asserts `parse_multi_reply_response("[]").is_none()`. In a live setting, this would occur if the AI model returns a valid but empty JSON array. The handler maps `None` from the parser to 502 Bad Gateway via `.ok_or(StatusCode::BAD_GATEWAY)`.
