# GC-219: Happy Path — Tasks Extracted from Thread

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: happy-path, thread, tasks, ai, extract
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled (`ai_enabled = "true"` in config table)

### Data
- At least one synced email thread with actionable content (e.g., "Please review the PR by Friday and send the updated report to the team") (source: inbox sync)
- Thread ID of that thread (source: `GET /api/threads` or `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify a thread with actionable content
   - **Target**: `GET http://localhost:3030/api/messages?limit=10`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with list of messages; pick a `thread_id` from a message whose body contains task-like language

3. Extract tasks from the thread
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with JSON body `{"tasks": [...]}` where `tasks` is a non-empty array

4. Validate task structure
   - **Target**: Response from step 3
   - **Input**: Inspect each element of `tasks` array
   - **Expected**: Each task has `task` (string, non-empty), `priority` (one of "high", "medium", "low"), `deadline` (string or null), `source_subject` (string or null)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `tasks` array with at least one element
- [ ] Each task has a non-empty `task` string
- [ ] Each task has `priority` set to "high", "medium", or "low"
- [ ] Each task has `deadline` as a string or null
- [ ] Each task has `source_subject` as a string or null

## Failure Criteria
- Response status is not 200
- `tasks` array is empty when the thread clearly contains action items
- Task fields are missing or have unexpected types

## Notes
This is the primary happy-path test. The AI provider must be functioning and the thread must contain recognizable action items. If the AI returns no tasks despite actionable content, that indicates a prompt or model issue rather than an API bug.
