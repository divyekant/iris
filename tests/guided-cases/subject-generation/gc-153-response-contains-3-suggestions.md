# GC-153: Response Contains Array of 3 Suggestions

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai, response-shape, suggestions-count, api
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

2. Submit a well-formed suggest-subject request
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "I am writing to propose a revised schedule for the product launch. Given the delays in QA, I recommend pushing the date by two weeks to ensure quality."}`
   - **Expected**: 200 with `{"suggestions": [...]}` where suggestions array length is exactly 3

3. Inspect each suggestion element
   - **Target**: same response body from step 2
   - **Input**: none
   - **Expected**: Each of the 3 elements is a non-empty string, distinct from the others, and plausibly relevant to the email body

## Success Criteria
- [ ] Response status is 200
- [ ] `suggestions` contains exactly 3 items
- [ ] All 3 items are non-empty strings
- [ ] All 3 items are distinct (no duplicates)
- [ ] Suggestions are semantically related to the provided body text

## Failure Criteria
- `suggestions` array has fewer than 3 items
- `suggestions` array has more than 3 items (contract violation)
- Any item is null, empty, or a duplicate
- Response is not 200
