# GC-285: Suggestions Capped at 3

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, cap, max-suggestions
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled

### Data
- At least one synced thread with substantial body text (to maximize AI suggestion diversity)
- `thread_id` from a multi-message thread (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Retrieve a thread ID from a message-rich thread
   - **Target**: `GET http://localhost:3030/api/messages?limit=10`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/messages?limit=10" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK; select a `thread_id` with multiple messages for richer context

3. Request autocomplete with a prompt likely to elicit many completions
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "<thread_id_from_step_2>",
         "partial_text": "I wanted to let you know that",
         "cursor_position": 29,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK with `suggestions` array containing at most 3 elements

4. Count suggestions and verify cap
   - **Target**: Response from step 3
   - **Input**: Count elements in `suggestions` array
   - **Expected**: `suggestions.length` is in range [0, 3] — never exceeds 3 regardless of AI output

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions` array has at most 3 elements
- [ ] All returned suggestions have `text`, `full_sentence`, and `confidence`
- [ ] `confidence` values are sorted descending
- [ ] `debounce_ms` is present

## Failure Criteria
- `suggestions` array contains more than 3 elements
- Any suggestion is missing required fields
- Non-200 status code
- `debounce_ms` absent

## Notes
The API spec mandates a maximum of 3 suggestions. If the underlying AI model produces more, the server must truncate to the top 3 by confidence before returning. This test confirms the cap is enforced server-side, not just by convention.
