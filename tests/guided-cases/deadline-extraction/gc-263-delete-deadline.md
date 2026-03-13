# GC-263: Delete a Deadline

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, delete, lifecycle
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- At least one extracted deadline in the `deadlines` table (source: prior `POST /api/ai/extract-deadlines` run)
- The `id` of that deadline (source: response from extraction or `GET /api/deadlines`)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract a deadline to obtain a known deadline ID
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<valid_message_id>"}'
     ```
   - **Expected**: 200 OK; capture the `id` of a deadline from the response

3. Delete the deadline
   - **Target**: `DELETE http://localhost:3030/api/deadlines/{id}`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X DELETE \
       "http://localhost:3030/api/deadlines/<deadline_id>" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK (or 204 No Content)

4. Confirm the deadline no longer appears in the list
   - **Target**: `GET http://localhost:3030/api/deadlines?include_completed=true`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?include_completed=true" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: The deleted deadline's `id` does not appear in the response

5. Attempt to delete the same deadline again
   - **Target**: `DELETE http://localhost:3030/api/deadlines/{id}`
   - **Input**:
     ```
     curl -s -o /dev/null -w "%{http_code}" -X DELETE \
       "http://localhost:3030/api/deadlines/<deadline_id>" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 404 Not Found (the row no longer exists)

## Success Criteria
- [ ] Initial `DELETE` request returns 200 or 204
- [ ] Deleted deadline is absent from all subsequent `GET /api/deadlines` responses (with and without `include_completed=true`)
- [ ] Second `DELETE` on the same ID returns 404

## Failure Criteria
- Initial `DELETE` returns a non-2xx status for a valid ID
- Deleted deadline still appears in the listing
- Second `DELETE` returns 200 or 204 instead of 404 (phantom delete)

## Notes
Idempotency check in step 5 validates that the handler does not silently succeed on a nonexistent row. A 404 on the second delete is the correct behavior.
