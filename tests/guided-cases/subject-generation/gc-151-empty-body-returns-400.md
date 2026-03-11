# GC-151: Empty Body Returns 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, validation, negative, 400, empty-body, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Request subject suggestions with an empty string body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": ""}`
   - **Expected**: 400 Bad Request with a JSON error message indicating body must not be empty

3. Request subject suggestions with body field omitted entirely
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{}`
   - **Expected**: 400 Bad Request with a JSON error message

4. Request subject suggestions with body set to whitespace only
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "   "}`
   - **Expected**: 400 Bad Request (whitespace-only body treated as empty)

## Success Criteria
- [ ] Empty string body returns 400
- [ ] Missing body field returns 400
- [ ] Whitespace-only body returns 400 (or at minimum no 200 with suggestions)
- [ ] Response body contains a descriptive error message (not a generic 400)
- [ ] No AI call is made (no latency spike expected for validation rejections)

## Failure Criteria
- Any of the invalid inputs returns 200 with suggestions
- Server returns 500 instead of 400
- Error response body is empty or unparseable JSON
