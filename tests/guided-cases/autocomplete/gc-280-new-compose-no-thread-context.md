# GC-280: Autocomplete with New Compose (No Thread Context)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, new-compose, no-thread
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled (`ai_enabled = "true"` in config table)

### Data
- No thread context required — `thread_id` is omitted or null to simulate composing a new email

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request autocomplete for a new compose with no thread_id
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "I wanted to follow up on our discussion about",
         "cursor_position": 47,
         "compose_mode": "new"
       }'
     ```
   - **Expected**: 200 OK with `suggestions` array (may be smaller or generic given no thread context) and `debounce_ms`

3. Confirm suggestions are still valid autocomplete completions
   - **Target**: Response from step 2
   - **Input**: Inspect each suggestion
   - **Expected**: Each suggestion has `text`, `full_sentence`, and `confidence`; suggestions read as plausible sentence completions even without thread context

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `suggestions` array (may be 0–3 elements)
- [ ] Response body contains `debounce_ms` positive integer
- [ ] If suggestions are present, each has `text`, `full_sentence`, and `confidence`
- [ ] Server does not error when `thread_id` is absent
- [ ] `confidence` values are floats in [0.0, 1.0]

## Failure Criteria
- Non-200 status code when `thread_id` is omitted
- 400 or 422 error treating missing `thread_id` as required field (it is optional for new compose)
- Response body missing `suggestions` or `debounce_ms`
- Server crash or 500 error when no thread context is provided

## Notes
`thread_id` is optional — new compose mode has no associated thread. The AI must gracefully generate generic completions from the `partial_text` alone.
