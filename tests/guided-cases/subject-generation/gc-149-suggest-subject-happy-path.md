# GC-149: Suggest Subject — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai, happy-path, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- No existing subject required

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Request must originate from http://127.0.0.1:3000 (or set `Sec-Fetch-Site: same-origin`)
   - **Expected**: 200 with JSON body containing `token` field

2. Request subject suggestions with a non-empty body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "Hi Sarah, I wanted to follow up on our meeting last Tuesday about the Q2 roadmap. Could you share the updated timeline with me when you get a chance?"}`
   - **Expected**: 200 with JSON body `{"suggestions": [...]}` where suggestions is a non-empty array

## Success Criteria
- [ ] Response status is 200
- [ ] Response Content-Type is application/json
- [ ] `suggestions` field is present and is an array
- [ ] Array contains at least 1 string element
- [ ] Each suggestion is a non-empty string
- [ ] No server error (no 500)

## Failure Criteria
- Response status is not 200
- `suggestions` key is absent from the response body
- `suggestions` is not an array
- Any suggestion element is null, empty, or not a string
- Server returns 500
