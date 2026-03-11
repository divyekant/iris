# GC-152: Body Exceeding 50 KB Returns 400

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, validation, negative, 400, size-limit, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A string of exactly 51,200 bytes (51 KB) and one of exactly 51,201 bytes (boundary + 1)

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Generate an oversized body payload (51,200 characters of "A") and submit
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "<51200 'A' characters>"}` (total body field value = 51 200 bytes of UTF-8)
   - **Expected**: 400 Bad Request with a JSON error message referencing the body size limit

3. Verify a body at exactly 50 KB (51,200 – 1 = 51,199 bytes) is accepted
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "<51199 'A' characters>"}`
   - **Expected**: 200 with suggestions array (or any non-400 success response)

## Success Criteria
- [ ] 51 200-byte body returns 400
- [ ] Error message mentions size limit or body too large
- [ ] 51 199-byte body is not rejected with 400 on size grounds
- [ ] Server does not crash (no 500) for either payload

## Failure Criteria
- Oversized body returns 200 with suggestions
- Server returns 500 instead of 400 for oversized body
- Boundary-case body (at limit) is rejected
