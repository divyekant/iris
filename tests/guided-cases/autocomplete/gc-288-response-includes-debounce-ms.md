# GC-288: Response Includes debounce_ms Hint

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, debounce, response-schema
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled

### Data
- No specific data required — `debounce_ms` is always present regardless of suggestions content

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request autocomplete with a valid partial_text in reply mode
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "Just wanted to confirm",
         "cursor_position": 22,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK; response JSON contains both `suggestions` and `debounce_ms`

3. Verify debounce_ms value equals 300
   - **Target**: Response from step 2
   - **Input**: Read `debounce_ms` field
   - **Expected**: `debounce_ms` is the integer `300`

4. Request autocomplete with a forward compose mode and confirm debounce_ms is consistent
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "FYI — please see the thread below regarding",
         "cursor_position": 43,
         "compose_mode": "forward"
       }'
     ```
   - **Expected**: 200 OK; `debounce_ms` is again `300` — value is stable across compose modes

## Success Criteria
- [ ] Response status is 200
- [ ] `debounce_ms` field is present in the response body
- [ ] `debounce_ms` value is a positive integer
- [ ] `debounce_ms` equals 300 (the documented default)
- [ ] `debounce_ms` is consistent across `reply` and `forward` compose modes
- [ ] `suggestions` field is also present in all responses

## Failure Criteria
- `debounce_ms` absent from response
- `debounce_ms` is 0, negative, or not an integer
- `debounce_ms` varies unexpectedly across requests with the same configuration
- Non-200 status code

## Notes
`debounce_ms` is a client-side hint telling the compose UI how long to wait after a keystroke before firing the next autocomplete request. The documented value is 300ms. This field must always be present — even when `suggestions` is empty — so the client can safely read it without null-checking.
