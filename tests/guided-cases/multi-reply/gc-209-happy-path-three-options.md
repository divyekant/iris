# GC-209: Happy path — multi-reply returns 3 tone options

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: multi-reply
- **Tags**: happy-path, formal, casual, brief, ai-generation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and enabled (`ai_enabled = "true"` in config table)

### Data
- Existing thread with at least one message (source: synced inbox or seed data)
- Known `thread_id` for that thread

## Steps
1. Send multi-reply request with valid thread_id
   - **Target**: `POST /api/ai/multi-reply`
   - **Input**: `{ "thread_id": "<valid_thread_id>" }`
   - **Expected**: 200 OK, response body `{ "options": [...] }` with exactly 3 elements

2. Validate each reply option structure
   - **Target**: Response JSON
   - **Input**: Inspect each element of `options` array
   - **Expected**: Each element has `tone` (string), `subject` (string), and `body` (string) fields

3. Verify tone coverage
   - **Target**: Response JSON
   - **Input**: Collect all `tone` values
   - **Expected**: The three tones are `"formal"`, `"casual"`, and `"brief"` (one of each)

4. Verify subject lines are reply subjects
   - **Target**: Response JSON
   - **Input**: Inspect `subject` on each option
   - **Expected**: Each subject is a non-empty string (typically prefixed with "Re:")

5. Verify body text is non-empty and distinct
   - **Target**: Response JSON
   - **Input**: Compare `body` values across options
   - **Expected**: All three bodies are non-empty strings; no two bodies are identical

## Success Criteria
- [ ] Response status is 200
- [ ] `options` array has exactly 3 elements
- [ ] Each option has `tone`, `subject`, and `body` as non-empty strings
- [ ] Tones cover formal, casual, and brief
- [ ] Body text differs across the three options

## Failure Criteria
- Non-200 status code
- Fewer or more than 3 options returned
- Missing or empty fields on any option
- Duplicate tone values

## Notes
This is the primary happy path. Validates the end-to-end flow from API request through AI generation to structured response parsing.
