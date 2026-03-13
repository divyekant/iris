# GC-227: Task without Deadline — Null Deadline Field

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: no-deadline, null-field, task-field, ai
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email with a task that has no deadline (e.g., "Can you look into the performance issues?" or "Let me know your thoughts on the new design") (source: inbox sync)
- The thread_id or message_id of that email (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify an email with an open-ended request (no deadline)
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; find a message with action items but no specific due dates

3. Extract tasks from the email
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with tasks where at least one has `deadline: null`

4. Validate the deadline field is null
   - **Target**: Response from step 3
   - **Input**: Inspect `deadline` field of returned tasks
   - **Expected**: At least one task has `"deadline": null` (not an empty string, not omitted)

## Success Criteria
- [ ] Response status is 200
- [ ] At least one task is extracted (non-empty array)
- [ ] At least one task has `deadline` explicitly set to `null`
- [ ] Task structure is otherwise complete (task, priority, source_subject present)

## Failure Criteria
- AI hallucinates a deadline where none exists in the email
- `deadline` is an empty string instead of null
- `deadline` field is missing entirely from the JSON object

## Notes
The `ExtractedTask` struct defines `deadline: Option<String>`, which serializes as `null` when `None`. The AI system prompt instructs the model to use `"null"` for tasks without deadlines. If the model returns the string `"null"` instead of JSON null, serde should still handle it (though it would be stored as `Some("null")` rather than `None`).
