# GC-264: Extract from Email with No Deadlines Returns Empty Array

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, ai, extraction, empty-result, no-deadlines
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email with purely informational or conversational content (e.g., a newsletter, a "thanks for your order" confirmation, or "Sounds good, talk soon") — no action items or dates mentioned (source: inbox sync)
- The `message_id` of that email (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Identify an email with no deadline language
   - **Target**: `GET http://localhost:3030/api/messages?limit=20`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/messages?limit=20" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK; select a `message_id` from a promotional or purely informational message

3. Extract deadlines from the deadline-free email
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<informational_message_id>"}'
     ```
   - **Expected**: 200 OK with JSON body `{"deadlines": []}` — an empty array

4. Verify response structure
   - **Target**: Response from step 3
   - **Input**: Inspect JSON body
   - **Expected**: Body has a `deadlines` key whose value is `[]`; no error fields; no 4xx status

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `deadlines` key
- [ ] `deadlines` is an empty array `[]`
- [ ] No hallucinated deadlines appear despite no date language in the email
- [ ] Response does not include an error or 4xx/5xx status

## Failure Criteria
- Response status is not 200
- `deadlines` contains fabricated deadlines from a purely informational email (model hallucination — note as model quality issue, not API bug)
- `deadlines` key is absent from the response body
- Response returns 404 (the message exists; this should be 200 with empty array, not 404)

## Notes
This case distinguishes between a nonexistent message (404, GC-261) and a valid message with no deadlines (200 with `[]`). The AI should correctly recognize the absence of deadline language. Hallucinated deadlines are a model quality concern but should be noted separately from structural failures.
