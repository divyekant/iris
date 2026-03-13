# GC-386: Negative — invalid formality value returns 400

## Metadata
- **Type**: negative
- **Priority**: P1
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
1. Send translate request with an unrecognized formality value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Could you send me the report?", "target_language": "Spanish", "formality": "very_formal"}`
   - **Expected**: 400 Bad Request with an error message indicating the formality value is invalid; accepted values are `formal`, `informal`, `neutral`

2. Send translate request with a boolean formality value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Hello.", "target_language": "French", "formality": true}`
   - **Expected**: 400 Bad Request (wrong type for `formality` field)

3. Send translate request with uppercase formality value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "See you soon.", "target_language": "German", "formality": "FORMAL"}`
   - **Expected**: 400 Bad Request (case-sensitive validation) or 200 OK if the API normalises case; document observed behaviour

4. Verify valid formality values work after invalid attempts
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Could you send me the report?", "target_language": "Spanish", "formality": "formal"}`
   - **Expected**: 200 OK with `translated_text` in Spanish

## Success Criteria
- [ ] `"very_formal"` returns 400
- [ ] Boolean formality value returns 400
- [ ] Error responses name the accepted formality values (`formal`, `informal`, `neutral`)
- [ ] Valid formality `"formal"` returns 200 after the invalid attempts

## Failure Criteria
- Server accepts unrecognised formality strings and returns 200
- Server returns 500 for an invalid formality type
- Valid formality values fail after prior invalid requests
- Error responses are missing or uninformative
