# GC-150: Suggest Subject Improvements When Current Subject Provided

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai, current-subject, improvement, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A non-empty existing subject string

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Request subject suggestions providing both body and current_subject
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "Just wanted to let you know the deployment is done and everything is green. The new billing module is live as of 14:00 UTC.", "current_subject": "update"}`
   - **Expected**: 200 with `{"suggestions": [...]}` containing refined or alternative subjects relative to the vague original

3. Verify the current_subject field is accepted without error
   - **Target**: same response as step 2
   - **Input**: none (inspect step 2 response)
   - **Expected**: No 400 or 422 error; response structure matches happy path (suggestions array)

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions` is a non-empty array of strings
- [ ] The endpoint does not reject the `current_subject` field
- [ ] Suggestions are contextually related to the provided body
- [ ] No server error

## Failure Criteria
- Response status is 400 or 422 when `current_subject` is included
- `suggestions` is empty or absent
- Server returns 500
