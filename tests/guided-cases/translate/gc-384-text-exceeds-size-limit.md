# GC-384: Negative — text exceeding 50,000 bytes returns 400 or 413

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, validation, negative, size-limit
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Send translate request with text exceeding 50,000 bytes
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "<string of 50,001+ characters — e.g., repeat 'a' * 51000>", "target_language": "Spanish"}`
   - **Expected**: 400 Bad Request or 413 Payload Too Large with an error message referencing the size limit

2. Verify no translation is performed
   - **Target**: Response body from step 1
   - **Input**: n/a
   - **Expected**: No `translated_text` field in the response; only an error message

3. Verify text at exactly 50,000 bytes is accepted
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "<string of exactly 50,000 characters>", "target_language": "French"}`
   - **Expected**: 200 OK (boundary value is within limit)

4. Verify server remains healthy after oversized request
   - **Target**: `GET /api/health`
   - **Input**: n/a
   - **Expected**: 200 OK; server has not crashed or entered an error state

## Success Criteria
- [ ] Request with 50,001+ bytes returns 400 or 413
- [ ] No translation results are returned for the oversized payload
- [ ] Error response includes a message referencing size or payload limits
- [ ] Request with exactly 50,000 bytes returns 200
- [ ] Server health check returns 200 after the oversized request

## Failure Criteria
- Server returns 200 and processes the oversized payload
- Server returns 500 or becomes unresponsive
- Boundary value (50,000 bytes) is rejected
- No error message about size in the rejection response
