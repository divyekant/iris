# GC-387: Negative — translate-email with non-existent message_id returns 404

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, validation, negative, 404, translate-email
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- A message ID that does not exist in the local database (e.g., a fabricated UUID)

## Steps
1. Attempt to translate an email with a non-existent message_id
   - **Target**: `POST /api/ai/translate-email`
   - **Input**: `{"message_id": "nonexistent-message-id-00000000", "target_language": "Spanish"}`
   - **Expected**: 404 Not Found with an error message indicating the message was not found

2. Attempt with a syntactically valid but absent UUID
   - **Target**: `POST /api/ai/translate-email`
   - **Input**: `{"message_id": "00000000-0000-0000-0000-000000000000", "target_language": "French"}`
   - **Expected**: 404 Not Found; no translation result in the response body

3. Attempt with an empty message_id string
   - **Target**: `POST /api/ai/translate-email`
   - **Input**: `{"message_id": "", "target_language": "German"}`
   - **Expected**: 400 Bad Request (empty ID is invalid input, not a lookup failure)

4. Attempt with missing message_id field
   - **Target**: `POST /api/ai/translate-email`
   - **Input**: `{"target_language": "Italian"}`
   - **Expected**: 400 Bad Request or 422 Unprocessable Entity

## Success Criteria
- [ ] Non-existent message ID returns 404
- [ ] UUID-format but absent message ID returns 404
- [ ] Empty message ID returns 400
- [ ] Missing message ID field returns 400 or 422
- [ ] All error responses include a descriptive message; no translated content is returned

## Failure Criteria
- Server returns 200 and attempts to translate a non-existent message
- Server returns 500 for any of these inputs
- Error responses are empty or generic (no message body)
