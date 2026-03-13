# GC-216: AI response wrapped in markdown fences is parsed correctly

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: markdown-fences, json-parsing, ai-response-format
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
1. Send multi-reply request to trigger AI generation
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>" }`
   - **Expected**: 200 OK regardless of whether AI wraps response in markdown fences

2. Verify response is valid even if AI used markdown fences
   - **Target**: Response JSON
   - **Input**: Inspect `options` array
   - **Expected**: Exactly 3 valid reply options with tone, subject, and body — no raw markdown fence artifacts in any field

## Success Criteria
- [ ] Response status is 200
- [ ] `options` array has 3 elements
- [ ] No option contains markdown fence strings (` ``` `) in tone, subject, or body
- [ ] All fields are clean, parsed JSON values

## Failure Criteria
- 502 Bad Gateway (indicates parse failure on markdown-wrapped response)
- Options contain raw markdown artifacts
- Empty options array

## Notes
The `parse_multi_reply_response` function handles three formats: (1) clean JSON array, (2) markdown-fenced JSON (strips ` ```json ` / ` ``` ` wrappers), and (3) JSON embedded in prose (extracts `[...]` substring). This case exercises path (2). The unit test `test_parse_multi_reply_response_markdown_wrapped` covers the parser directly; this guided case validates it end-to-end through the API.
