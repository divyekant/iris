# GC-284: Unknown thread_id Still Returns Suggestions

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, edge, unknown-thread, graceful-degradation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and enabled

### Data
- A `thread_id` value that does not exist in the database (source: inline, e.g., `"thread-does-not-exist-xyz-99999"`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request autocomplete with a non-existent thread_id
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "thread_id": "thread-does-not-exist-xyz-99999",
         "partial_text": "Thanks for your patience, I wanted to",
         "cursor_position": 37,
         "compose_mode": "reply"
       }'
     ```
   - **Expected**: 200 OK; `suggestions` array may be empty or contain context-free completions; NOT a 404 error

3. Verify response structure is intact despite missing context
   - **Target**: Response from step 2
   - **Input**: Inspect JSON body
   - **Expected**: `suggestions` key present (array, possibly empty), `debounce_ms` present and positive

## Success Criteria
- [ ] Response status is 200 (not 404)
- [ ] `suggestions` array is present (empty or with context-free completions)
- [ ] `debounce_ms` is present and positive
- [ ] Server gracefully degrades — falls back to context-free suggestions when thread context is unavailable

## Failure Criteria
- 404 Not Found returned for unknown `thread_id`
- 500 Internal Server Error
- Response missing `suggestions` or `debounce_ms`
- Server panics or returns an error body

## Notes
Autocomplete is a best-effort feature. If the thread context cannot be loaded (unknown ID), the server should degrade gracefully to context-free suggestions rather than erroring. This mirrors behavior expected from a reply being composed before full sync completes.
