# GC-261: Extract Deadlines with Unknown Message ID Returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, ai, extraction, 404, not-found, invalid-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A message_id guaranteed not to exist in the database: `nonexistent-message-id-99999` (source: inline)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Attempt to extract deadlines using a nonexistent message_id
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "nonexistent-message-id-99999"}'
     ```
   - **Expected**: 404 Not Found

3. Verify the response body does not expose internal details
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "nonexistent-message-id-99999"}'
     ```
   - **Expected**: Response body is a JSON error object (e.g., `{"error": "message not found"}`) without database paths, stack traces, or internal identifiers

## Success Criteria
- [ ] Response status is 404 Not Found
- [ ] Response body is valid JSON with an error message
- [ ] Response body does not contain SQL fragments, file paths, or stack traces
- [ ] The AI provider is not invoked (no AI call made for a nonexistent message)

## Failure Criteria
- Response status is 200 with an empty `deadlines` array (should be 404, not 200 with [])
- Response status is 500 Internal Server Error
- Response body exposes internal database details or stack traces
- AI provider is contacted despite no message being found

## Notes
The handler must perform a database lookup for the `message_id` before invoking AI. If the message does not exist, the request should fail fast with 404. Returning 200 with an empty array would be incorrect — that response is reserved for messages that exist but contain no deadlines (see GC-264).
