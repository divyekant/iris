# GC-282: Very Short partial_text (Under 3 Characters)

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, edge, short-input
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled

### Data
- No specific data required

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request autocomplete with a single-character partial_text
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "H",
         "cursor_position": 1,
         "compose_mode": "new"
       }'
     ```
   - **Expected**: 200 OK; `suggestions` may be empty `[]` or contain low-confidence generic completions; no error

3. Request autocomplete with a two-character partial_text
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "Hi",
         "cursor_position": 2,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK; `suggestions` may be empty or contain very short completions; `debounce_ms` present

## Success Criteria
- [ ] Response status is 200 for both 1-char and 2-char inputs
- [ ] `suggestions` array is present (empty or non-empty)
- [ ] `debounce_ms` is present and positive
- [ ] No 400, 422, or 500 errors for short-but-non-empty input
- [ ] If suggestions are returned, they have valid `text`, `full_sentence`, and `confidence` fields

## Failure Criteria
- 400 or 422 error rejecting short input as invalid
- 500 Internal Server Error
- `suggestions` key absent from response
- `debounce_ms` absent from response

## Notes
Very short inputs offer insufficient context for quality suggestions. The server should handle them gracefully — either by returning empty suggestions or low-confidence completions — without erroring. The debounce hint (300ms default) guides the client to wait for more typing before calling.
