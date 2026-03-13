# GC-383: Negative — empty text returns 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, validation, negative, 400
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Send translate request with empty text string
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "", "target_language": "Japanese"}`
   - **Expected**: 400 Bad Request with error message indicating text is required or cannot be empty

2. Send translate request with missing text field entirely
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"target_language": "Spanish"}`
   - **Expected**: 400 Bad Request (or 422 Unprocessable Entity)

3. Send translate request with whitespace-only text
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "   ", "target_language": "French"}`
   - **Expected**: 400 Bad Request; server treats blank text as invalid input

4. Send detect-language request with empty text
   - **Target**: `POST /api/ai/detect-language`
   - **Input**: `{"text": ""}`
   - **Expected**: 400 Bad Request with a meaningful error message

## Success Criteria
- [ ] Empty string returns 400
- [ ] Missing `text` field returns 400 or 422
- [ ] Whitespace-only text returns 400
- [ ] detect-language with empty text returns 400
- [ ] All error responses include a human-readable error message

## Failure Criteria
- Any request returns 200 and performs a translation
- Server returns 500 for empty or missing input
- Error responses lack a descriptive message
