# GC-250: Get Stored Intent from Message

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, stored, get
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A message ID for which intent has already been detected and stored (source: run GC-249 first, or use a message processed by the AI pipeline)
- The `intent` value returned from the prior detection run (for cross-validation)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Detect intent for a message to ensure it is stored
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK with `{"intent": "$DETECTED_INTENT", "confidence": <float>}`; record `$DETECTED_INTENT`

3. Retrieve the stored intent via GET endpoint
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID/intent`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/messages/$MESSAGE_ID/intent \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON body containing the stored intent, e.g., `{"intent": "$DETECTED_INTENT", "confidence": <float>}` or at minimum `{"intent": "$DETECTED_INTENT"}`

4. Confirm the GET response matches the POST response
   - **Target**: Compare step 3 response with step 2 response
   - **Input**: n/a
   - **Expected**: `intent` value returned by GET matches `intent` value returned by POST (same classification, reading from the `intent` column on the messages table)

5. Confirm the GET is idempotent — call it a second time
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID/intent`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: Same 200 OK response with the same `intent` value; stored value does not change between reads

## Success Criteria
- [ ] GET endpoint returns 200 for a message that has a stored intent
- [ ] `intent` field is present in the GET response
- [ ] `intent` value matches what was returned by the earlier POST detect-intent call
- [ ] Second GET call returns identical data (idempotent read)

## Failure Criteria
- GET returns 404 even though intent was previously stored via POST
- `intent` value from GET does not match `intent` value from POST
- GET returns null for a message with a stored intent
- Any non-200 status code on either GET call
