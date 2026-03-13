# GC-259: Happy Path — Extract Deadlines from Email with Explicit Date

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, ai, extraction, happy-path, explicit-date
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled (`ai_enabled = "true"` in config table)

### Data
- A synced email whose body contains an explicit deadline (e.g., "Please submit your expense report by March 20th" or "The proposal is due on Friday, March 15") (source: inbox sync)
- The `message_id` of that email (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify an email with an explicit deadline
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**:
     ```
     curl -s http://localhost:3030/api/messages?limit=20 \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK; select a `message_id` from a message whose subject or body contains a specific date or deadline phrase

3. Extract deadlines from the email
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_id_from_step_2>"}'
     ```
   - **Expected**: 200 OK with JSON body `{"deadlines": [...]}` where `deadlines` is a non-empty array

4. Validate deadline object structure
   - **Target**: Response from step 3
   - **Input**: Inspect each element of the `deadlines` array
   - **Expected**: Each deadline object has:
     - `description` (non-empty string describing what is due)
     - `deadline_date` (string representing the parsed date)
     - `deadline_source` (string quoting the source text from the email)
     - `is_explicit` (boolean, `true` for explicitly-stated dates)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains a `deadlines` key
- [ ] `deadlines` array has at least one element
- [ ] Each deadline has `description` as a non-empty string
- [ ] Each deadline has `deadline_date` as a non-null string
- [ ] Each deadline has `deadline_source` as a non-empty string referencing the original email text
- [ ] At least one deadline has `is_explicit: true`

## Failure Criteria
- Response status is not 200
- `deadlines` array is empty when the email clearly states a due date
- Any deadline is missing required fields (`description`, `deadline_date`, `deadline_source`, `is_explicit`)
- `is_explicit` is `false` when the date is stated literally in the email

## Notes
This is the primary happy-path test. The AI model must be functional and the target email must contain unambiguous deadline language. If the model returns an empty array despite explicit deadline phrasing, this indicates a prompt or model quality issue rather than an API bug.
