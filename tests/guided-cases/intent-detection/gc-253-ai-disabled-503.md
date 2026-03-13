# GC-253: Detect Intent When AI Is Disabled Returns 503

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, ai-disabled, 503, service-unavailable
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI is **disabled**: either `ai_enabled` config key set to `"false"` or all AI providers unavailable/unconfigured

### Data
- Any valid message ID that exists in the database (source: `GET /api/messages`)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Identify a valid message ID
   - **Target**: `GET http://localhost:3000/api/messages?limit=5`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK with at least one message; store any `id` as `$MESSAGE_ID`

3. Disable AI via settings API
   - **Target**: `PUT http://localhost:3000/api/settings`
   - **Input**:
     ```
     curl -s -X PUT http://localhost:3000/api/settings \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"ai_enabled": "false"}'
     ```
   - **Expected**: 200 OK; AI is now disabled

4. Attempt to detect intent with AI disabled
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -o - -w "\nHTTP_STATUS:%{http_code}" -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$MESSAGE_ID"}'
     ```
   - **Expected**: 503 Service Unavailable; response body may contain an error message such as `{"error": "AI is disabled"}` or similar

5. Verify no AI provider call was attempted (server logs)
   - **Target**: Server logs or health endpoint
   - **Input**: n/a
   - **Expected**: No outbound AI request logged; the check short-circuits before fetching message content

6. Re-enable AI (cleanup)
   - **Target**: `PUT http://localhost:3000/api/settings`
   - **Input**:
     ```
     curl -s -X PUT http://localhost:3000/api/settings \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"ai_enabled": "true"}'
     ```
   - **Expected**: 200 OK; AI features restored

## Success Criteria
- [ ] Step 4 returns 503 Service Unavailable
- [ ] Response body contains an error message (not an intent value)
- [ ] No AI provider call is attempted while AI is disabled
- [ ] Server returns to normal after re-enabling AI in step 6

## Failure Criteria
- Returns 200 with a null or empty intent instead of 503
- Returns 500 (unhandled error rather than graceful 503)
- Returns 404 (message exists; the 404 check must come before the AI-disabled check, or AI-disabled must be 503 regardless of message existence)
- Response body leaks provider configuration details

## Notes
The AI-disabled check should occur early in the handler — after request validation but before message fetching. The expected order is: (1) validate request body (missing message_id → 400), (2) check AI enabled (disabled → 503), (3) look up message (not found → 404), (4) call provider.
