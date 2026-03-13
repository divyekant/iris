# GC-266: Duplicate Extraction Does Not Create Duplicate Deadlines

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, deduplication, idempotency, unique-constraint
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email with at least one clearly-stated deadline (source: inbox sync)
- The `message_id` of that email (source: `GET /api/messages`)
- The `thread_id` of the thread containing that email (source: `GET /api/messages`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract deadlines from the email for the first time
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_id>"}'
     ```
   - **Expected**: 200 OK; note the count of deadlines returned (e.g., N=1) and each deadline's `id`

3. Extract deadlines from the same email a second time
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_id>"}'
     ```
   - **Expected**: 200 OK; same count of deadlines (N) returned; same `id` values as step 2

4. List all deadlines for the containing thread
   - **Target**: `GET http://localhost:3030/api/threads/{thread_id}/deadlines`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/threads/<thread_id>/deadlines" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: The thread's deadline list contains exactly N unique entries — no duplicates despite two extraction runs

5. Verify unique constraint by counting by description
   - **Target**: Response from step 4
   - **Input**: Inspect the `description` values of all returned deadlines
   - **Expected**: No two deadlines share the same `description` for the same `message_id`

## Success Criteria
- [ ] Both extraction calls return 200
- [ ] Deadline count does not increase after the second extraction run
- [ ] Deadline `id` values are the same in both extraction responses
- [ ] Thread-scoped listing shows exactly N unique deadline rows, not 2×N
- [ ] The `unique(message_id, description)` constraint is enforced at the database level

## Failure Criteria
- Second extraction creates duplicate rows (count doubles from N to 2N)
- Different `id` values returned for the same deadline on successive extractions
- Thread listing shows duplicate entries with identical `description` and `message_id`

## Notes
The `deadlines` table has a `UNIQUE(message_id, description)` constraint. The extraction handler should use INSERT OR IGNORE (or equivalent upsert) so that re-running extraction on the same message is idempotent. This test validates that constraint is both present and enforced.
