# GC-262: Mark Deadline as Complete

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, complete, put, lifecycle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- At least one extracted deadline in the `deadlines` table with `completed_at` as NULL (source: prior `POST /api/ai/extract-deadlines` run)
- The `id` of that deadline (source: response from `GET /api/deadlines` or extraction response)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract a deadline to get a known deadline ID
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<valid_message_id>"}'
     ```
   - **Expected**: 200 OK; capture the `id` of the first deadline in the response

3. Mark the deadline as complete
   - **Target**: `PUT http://localhost:3030/api/deadlines/{id}/complete`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3030/api/deadlines/<deadline_id>/complete" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK with a response confirming the deadline is now marked complete (e.g., `{"id": "...", "completed_at": "2026-03-13T..."}`)

4. Confirm the deadline no longer appears in the default upcoming list
   - **Target**: `GET http://localhost:3030/api/deadlines`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: The completed deadline's `id` does not appear in the returned list (default `include_completed=false`)

5. Confirm the deadline appears when include_completed is true
   - **Target**: `GET http://localhost:3030/api/deadlines?include_completed=true`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?include_completed=true" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: The completed deadline's `id` appears in the list with a non-null `completed_at` timestamp

## Success Criteria
- [ ] `PUT /api/deadlines/{id}/complete` returns 200
- [ ] Response includes a non-null `completed_at` timestamp
- [ ] Completed deadline is absent from the default `GET /api/deadlines` listing
- [ ] Completed deadline appears in `GET /api/deadlines?include_completed=true`

## Failure Criteria
- `PUT` request returns a non-200 status for a valid deadline ID
- `completed_at` is null or missing in the response
- Completed deadline still appears in the default listing
- Completed deadline is absent even when `include_completed=true`
