# GC-226: Task with Deadline Populated

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: deadline, date, task-field, ai
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email thread or message whose body contains an explicit deadline (e.g., "Please submit the quarterly report by March 20th" or "Budget proposal due next Friday") (source: inbox sync)
- The thread_id or message_id of that email (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify an email with a deadline mention
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; find a message whose body mentions a specific date or deadline

3. Extract tasks from the email with a deadline
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with JSON body containing at least one task where `deadline` is a non-null string

4. Validate the deadline field
   - **Target**: Response from step 3
   - **Input**: Inspect the `deadline` field of tasks
   - **Expected**: At least one task has `deadline` as a non-null, non-empty string representing a date (e.g., "March 20", "2026-03-20", "next Friday")

## Success Criteria
- [ ] Response status is 200
- [ ] At least one task in the array has a non-null `deadline` field
- [ ] The deadline string corresponds to the date mentioned in the email body
- [ ] The `deadline` is a string type (not a number or boolean)

## Failure Criteria
- All tasks have `deadline: null` despite explicit deadline language in the email
- `deadline` field is missing from the response structure
- Response is not 200

## Notes
The AI model is instructed to return `"deadline": "date or null"`. The exact format of the date string is model-dependent (could be ISO 8601, natural language, etc.). The test validates the field is populated when a deadline is clearly stated, not the exact date format.
