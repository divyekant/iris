# GC-249: Happy Path — Detect Intent Returns Valid Classification

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin`)
- AI provider configured and enabled (`ai_enabled = "true"` in config table, at least one healthy provider)

### Data
- A known message ID from a synced inbox (source: `GET /api/messages`) — ideally a message with clear actionable text, e.g., "Please review the attached proposal and send me your comments by Thursday."

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field; store as `$TOKEN`

2. Pick a message ID from the inbox
   - **Target**: `GET http://localhost:3000/api/messages?limit=10`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK with `{"messages": [...]}` array; store any `id` as `$MESSAGE_ID`

3. POST to detect-intent with the message ID
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK with JSON body `{"intent": "<value>", "confidence": <float>}` where `intent` is one of `action_request`, `question`, `fyi`, `scheduling`, `sales`, `social`, `newsletter`

4. Validate that the `intent` value is a recognized enum member
   - **Target**: Response body from step 3
   - **Input**: n/a
   - **Expected**: `intent` is exactly one of the 7 valid values; no other strings are returned

5. Validate that `confidence` is present and numeric
   - **Target**: Response body from step 3
   - **Input**: n/a
   - **Expected**: `confidence` is a JSON number (not a string, not null)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains both `intent` and `confidence` fields
- [ ] `intent` is one of: `action_request`, `question`, `fyi`, `scheduling`, `sales`, `social`, `newsletter`
- [ ] `confidence` is a numeric value
- [ ] No 4xx or 5xx error is returned

## Failure Criteria
- Response status is not 200
- `intent` field is absent or null
- `intent` value is not one of the 7 valid enum members
- `confidence` field is absent or not numeric
- Server returns 503 despite AI being configured and healthy
