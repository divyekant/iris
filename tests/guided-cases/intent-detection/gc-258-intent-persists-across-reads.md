# GC-258: Intent Persists Across Reads After Detection

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, persistence, stored, reads
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A message ID from a synced inbox (source: `GET /api/messages`)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Detect intent for the message (first classification)
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK with `{"intent": "$INTENT_VALUE", "confidence": <float>}`; record `$INTENT_VALUE`

3. Read the stored intent immediately after detection
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID/intent`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/messages/$MESSAGE_ID/intent \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK; `intent` matches `$INTENT_VALUE` from step 2

4. Read the intent again after marking the message as read
   - **Target**:
     1. `PATCH http://localhost:3000/api/messages/$MESSAGE_ID` with `{"is_read": true}`
        ```
        curl -s -X PATCH http://localhost:3000/api/messages/$MESSAGE_ID \
          -H "x-session-token: $TOKEN" \
          -H "Content-Type: application/json" \
          -d '{"is_read": true}'
        ```
     2. `GET http://localhost:3000/api/messages/$MESSAGE_ID/intent`
   - **Expected**: PATCH returns 200; GET returns 200 with same `intent` = `$INTENT_VALUE` (marking read does not clear intent)

5. Verify intent appears in the message detail response
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; message detail object includes an `intent` field with value `$INTENT_VALUE`

6. Verify intent appears in the message list response
   - **Target**: `GET http://localhost:3000/api/messages?limit=20`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; the message entry in the `messages` array has `intent` field set to `$INTENT_VALUE` (if the list endpoint serializes the `intent` column)

7. Detect intent a second time for the same message (re-detection)
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK; `intent` value is the same as `$INTENT_VALUE` (deterministic for same content, or at minimum still a valid enum member); no 4xx or 5xx errors

## Success Criteria
- [ ] Intent value returned by POST detect-intent matches value returned by GET stored-intent (step 2 vs step 3)
- [ ] Marking a message as read does not change or clear its stored intent (step 4)
- [ ] Message detail endpoint (`GET /api/messages/{id}`) includes the `intent` field with the stored value (step 5)
- [ ] Re-detecting intent for the same message does not crash or return an error (step 7)
- [ ] All API calls return 200

## Failure Criteria
- GET stored-intent returns a different `intent` than what POST detect-intent returned
- Marking the message as read clears the `intent` column (unintended side effect)
- Message detail endpoint omits the `intent` field
- Re-detection returns 500 or 409 Conflict instead of overwriting or returning the stored value
- `intent` value changes to null after any operation other than explicit re-classification
