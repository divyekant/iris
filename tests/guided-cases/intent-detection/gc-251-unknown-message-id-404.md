# GC-251: Detect Intent with Unknown Message ID Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, not-found, 404
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A message ID that does not exist in the database (source: inline — fabricated, e.g., `nonexistent-message-id-xyz`)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. POST detect-intent with a fabricated, non-existent message ID
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "nonexistent-message-id-xyz"}'
     ```
   - **Expected**: 404 Not Found

3. Confirm the response body does not contain any message data
   - **Target**: Response body from step 2
   - **Input**: n/a
   - **Expected**: Body is an error object or empty; no `intent` or `confidence` fields; no data from other messages

4. Attempt GET for the same non-existent message ID
   - **Target**: `GET http://localhost:3000/api/messages/nonexistent-message-id-xyz/intent`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" \
       http://localhost:3000/api/messages/nonexistent-message-id-xyz/intent \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 404 Not Found

5. Verify server health after the failed requests
   - **Target**: `GET http://localhost:3000/api/health`
   - **Input**: n/a
   - **Expected**: 200 OK; server is stable

## Success Criteria
- [ ] POST detect-intent with unknown message ID returns 404
- [ ] GET stored intent with unknown message ID returns 404
- [ ] Response body contains no message data (no info leakage)
- [ ] Server remains healthy after the 404 responses

## Failure Criteria
- Either endpoint returns 200 with null or empty intent (should be 404, not a silent empty result)
- Either endpoint returns 500
- Response body exposes SQL error details or database structure
- Server becomes unhealthy after the requests
