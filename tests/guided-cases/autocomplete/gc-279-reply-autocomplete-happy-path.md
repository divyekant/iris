# GC-279: Happy Path — Autocomplete Returns Suggestions for Reply

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, reply, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled (`ai_enabled = "true"` in config table)

### Data
- At least one synced email thread exists in the database (source: prior sync)
- A valid `thread_id` from an existing thread (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Retrieve a thread ID from the inbox
   - **Target**: `GET http://localhost:3030/api/messages?limit=5`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/messages?limit=5" \
       -H "X-Session-Token: $TOKEN"
     ```
   - **Expected**: 200 OK; note `thread_id` from the first message in the response

3. Request autocomplete suggestions for a partial reply
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "<thread_id_from_step_2>",
         "partial_text": "Thanks for reaching out. I will",
         "cursor_position": 34,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK with JSON body containing `suggestions` array and `debounce_ms` field

4. Validate the suggestions array
   - **Target**: Response from step 3
   - **Input**: Inspect the `suggestions` array
   - **Expected**: Array contains 1–3 elements; each element has `text` (string), `full_sentence` (string), and `confidence` (float 0.0–1.0); array is sorted by `confidence` descending

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `suggestions` key (non-empty array)
- [ ] Response body contains `debounce_ms` key (positive integer)
- [ ] Each suggestion has `text`, `full_sentence`, and `confidence` fields
- [ ] `confidence` values are floats between 0.0 and 1.0 inclusive
- [ ] At most 3 suggestions returned
- [ ] Suggestions are sorted by `confidence` descending (highest first)

## Failure Criteria
- Non-200 status code
- `suggestions` key absent from response
- Any suggestion missing `text`, `full_sentence`, or `confidence`
- More than 3 suggestions returned
- `confidence` values outside [0.0, 1.0]
- `debounce_ms` absent or non-positive
