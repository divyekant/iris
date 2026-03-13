# GC-222: Extract Tasks with message_id Only

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: message-id, single-message, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email message with actionable content (source: inbox sync)
- The message's `id` field (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Get a message ID with actionable content
   - **Target**: `GET http://localhost:3030/api/messages?limit=10`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with messages list; pick an `id` from a message containing task-like language

3. Extract tasks using message_id only (no thread_id)
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"message_id": "{message_id}"}`
   - **Expected**: 200 OK with JSON body `{"tasks": [...]}` containing task objects

4. Verify tasks are scoped to the single message
   - **Target**: Response from step 3
   - **Input**: Inspect `source_subject` fields
   - **Expected**: If `source_subject` is populated, it matches the subject of the single message provided

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `tasks` array (may be empty if message has no action items)
- [ ] Each task in array has valid structure (task, priority, deadline, source_subject)
- [ ] Extraction is scoped to the single message, not the full thread

## Failure Criteria
- Response status is not 200
- Handler rejects request despite valid message_id
- Tasks reference subjects from other messages not included in the request

## Notes
When only `message_id` is provided, the handler fetches that single message via `MessageDetail::get_by_id` and passes `vec![msg]` to the prompt builder. This verifies the single-message code path works independently from the thread path.
