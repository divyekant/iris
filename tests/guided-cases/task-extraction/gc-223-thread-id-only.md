# GC-223: Extract Tasks with thread_id Only

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: thread-id, multi-message, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email thread with multiple messages and actionable content (source: inbox sync)
- The thread's `thread_id` (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify a multi-message thread
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; find a `thread_id` that appears on multiple messages (indicating a thread with replies)

3. Extract tasks using thread_id only (no message_id)
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with JSON body `{"tasks": [...]}` containing task objects

4. Verify tasks may reference multiple messages
   - **Target**: Response from step 3
   - **Input**: Inspect `source_subject` fields across returned tasks
   - **Expected**: Tasks reflect action items from across the thread, not just one message

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `tasks` array
- [ ] Tasks are extracted from the full thread context (all messages in thread)
- [ ] Each task has valid structure (task, priority, deadline, source_subject)

## Failure Criteria
- Response status is not 200
- Handler only processes the first message of the thread
- Request fails when only thread_id is provided

## Notes
When `thread_id` is provided, the handler fetches all messages via `MessageDetail::list_by_thread` and passes them all to `build_extract_tasks_prompt`. The prompt builder truncates each message body to 800 chars and caps total at 4000 chars.
