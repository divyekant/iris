# GC-281: Empty partial_text Returns No Suggestions

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, edge, empty-input
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

2. Request autocomplete with empty partial_text
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "",
         "cursor_position": 0,
         "compose_mode": "new"
       }'
     ```
   - **Expected**: 200 OK with `{"suggestions": [], "debounce_ms": 300}` — empty suggestions array, no error

3. Request autocomplete with whitespace-only partial_text
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "   ",
         "cursor_position": 3,
         "compose_mode": "new"
       }'
     ```
   - **Expected**: 200 OK with `{"suggestions": [], "debounce_ms": 300}` — whitespace treated as empty

## Success Criteria
- [ ] Response status is 200 for empty string input
- [ ] `suggestions` is an empty array `[]`
- [ ] `debounce_ms` is present and positive
- [ ] No AI call made (or AI call returns empty) — server does not error
- [ ] Whitespace-only input also yields empty suggestions (or 200 with empty array)

## Failure Criteria
- 400 or 422 error treating empty `partial_text` as invalid
- Server returns suggestions for an empty string
- 500 Internal Server Error
- `suggestions` key absent from response

## Notes
The compose UI should not trigger an AI call when the text box is empty — returning an empty suggestions array with the debounce hint allows the client to skip the call client-side. This test verifies the server-side safety net if the client still sends the request.
