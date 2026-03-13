# GC-286: AI Disabled Returns 503 Service Unavailable

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, ai-disabled, 503, service-unavailable
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider **disabled** (`ai_enabled = "false"` in config table, set via Settings or direct DB update)

### Data
- No thread or message data required — the response must be 503 before any AI or context fetch

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Disable AI via config endpoint
   - **Target**: `PUT http://localhost:3030/api/config`
   - **Input**:
     ```
     curl -s -X PUT http://localhost:3030/api/config \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"key": "ai_enabled", "value": "false"}'
     ```
   - **Expected**: 200 OK confirming config update

3. Request autocomplete while AI is disabled
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "I wanted to follow up",
         "cursor_position": 21,
         "compose_mode": "new"
       }'
     ```
   - **Expected**: 503 Service Unavailable with error body explaining AI is not available

4. Re-enable AI to restore environment
   - **Target**: `PUT http://localhost:3030/api/config`
   - **Input**:
     ```
     curl -s -X PUT http://localhost:3030/api/config \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"key": "ai_enabled", "value": "true"}'
     ```
   - **Expected**: 200 OK confirming config restored

## Success Criteria
- [ ] Response status is 503 when AI is disabled
- [ ] Error body is present and indicates AI is unavailable (not a generic 503)
- [ ] No suggestions are returned in the error response
- [ ] AI re-enable restores normal operation (subsequent requests return 200)

## Failure Criteria
- 200 OK returned with empty or fake suggestions when AI is disabled
- 500 Internal Server Error instead of 503
- 503 with no explanatory error body
- Server crashes when AI is disabled and autocomplete is requested

## Notes
This mirrors the pattern established in GC-213 (multi-reply AI disabled) and GC-225 (task extraction AI disabled). The 503 response communicates a temporary unavailability rather than a client error, allowing the frontend to show a graceful "AI unavailable" state and suppress the autocomplete UI.
