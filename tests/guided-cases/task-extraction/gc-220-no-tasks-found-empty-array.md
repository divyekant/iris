# GC-220: No Tasks Found — Empty Array Returned

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: empty-result, no-tasks, edge, ai
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email thread with purely informational content and no action items (e.g., a newsletter, FYI email, or "Thanks for the update!") (source: inbox sync)
- Thread ID of that thread (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify a thread with no actionable content
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; pick a `thread_id` from a message that is clearly informational (no requests, deadlines, or assignments)

3. Extract tasks from the informational thread
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with JSON body `{"tasks": []}` — an empty array

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `tasks` key
- [ ] `tasks` is an empty array `[]`
- [ ] No error or 4xx/5xx status

## Failure Criteria
- Response status is not 200
- `tasks` array contains hallucinated tasks from a purely informational email
- Response body is malformed or missing `tasks` key

## Notes
The AI model should correctly identify that the email has no action items and return `[]`. If the model hallucinates tasks, the test passes at the API level (200 with valid structure) but should be noted as a model quality issue.
