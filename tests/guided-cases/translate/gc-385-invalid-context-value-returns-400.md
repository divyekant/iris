# GC-385: Negative — invalid context value returns 400

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
1. Send translate request with an unrecognized context value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Please review the attached document.", "target_language": "German", "context": "sms_message"}`
   - **Expected**: 400 Bad Request with an error message indicating the context value is invalid; accepted values are `email_compose`, `casual`, `business`

2. Send translate request with a numeric context value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Hello there.", "target_language": "Italian", "context": 42}`
   - **Expected**: 400 Bad Request (wrong type for `context` field)

3. Send translate request with an empty string context value
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Let's schedule a call.", "target_language": "Portuguese", "context": ""}`
   - **Expected**: 400 Bad Request; empty string is not a valid context value

4. Verify valid context values still work after invalid attempts
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Let's schedule a call.", "target_language": "Portuguese", "context": "business"}`
   - **Expected**: 200 OK with `translated_text` in Portuguese

## Success Criteria
- [ ] Unknown context string returns 400
- [ ] Numeric context value returns 400
- [ ] Empty string context value returns 400
- [ ] Error responses include a message naming the accepted context values
- [ ] Valid context `"business"` returns 200 after the invalid attempts

## Failure Criteria
- Server accepts `"sms_message"` or other unknown context strings and returns 200
- Server returns 500 for an invalid context type
- Valid context values fail after an invalid request
- Error responses are missing or contain no descriptive message
