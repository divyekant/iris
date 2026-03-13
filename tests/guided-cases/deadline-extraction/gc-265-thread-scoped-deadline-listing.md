# GC-265: Thread-Scoped Deadline Listing

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, thread, scoped-listing, thread-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- Two distinct email threads (thread_A and thread_B), each with at least one message containing deadline language (source: inbox sync)
- Deadlines extracted from messages in both thread_A and thread_B (source: prior `POST /api/ai/extract-deadlines` runs)
- The `thread_id` values for both threads (source: `GET /api/messages` or `GET /api/threads`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract deadlines from a message in thread_A
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_in_thread_A>"}'
     ```
   - **Expected**: 200 OK with at least one deadline; note the returned deadline `id`

3. Extract deadlines from a message in thread_B
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_in_thread_B>"}'
     ```
   - **Expected**: 200 OK with at least one deadline

4. List deadlines scoped to thread_A only
   - **Target**: `GET http://localhost:3030/api/threads/{thread_A_id}/deadlines`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/threads/<thread_A_id>/deadlines" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON body `{"deadlines": [...]}` containing only deadlines from messages belonging to thread_A

5. Verify thread_B deadlines are absent from thread_A response
   - **Target**: Response from step 4
   - **Input**: Compare returned deadline `id` values against those extracted from thread_B in step 3
   - **Expected**: No deadline IDs from thread_B appear in the thread_A response

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `deadlines` array
- [ ] All returned deadlines belong to messages in thread_A
- [ ] Deadlines from thread_B are not included in the thread_A scoped response

## Failure Criteria
- Response status is not 200
- Deadlines from thread_B appear in the thread_A scoped listing
- `deadlines` key is missing from the response body
- Thread endpoint returns all deadlines rather than thread-scoped ones

## Notes
The `GET /api/threads/{id}/deadlines` endpoint must join the `deadlines` table with `messages` filtered by `thread_id`. This test validates the scoping SQL join is correct. See also GC-260 for the unscoped listing.
