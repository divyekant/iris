# GC-268: Include_completed Parameter Shows Completed Deadlines

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, include-completed, query-param, filter
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- At least one extracted deadline that has been marked complete via `PUT /api/deadlines/{id}/complete` (source: prior run or setup via GC-262)
- At least one extracted deadline that is still incomplete (source: prior extraction run)
- Both deadlines have `deadline_date` values within the next 30 days to avoid date-window filtering

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract deadlines and capture IDs for both a near-future and a completed deadline
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_with_deadline>"}'
     ```
   - **Expected**: 200 OK with at least one deadline; note the `id`

3. Mark one deadline as complete
   - **Target**: `PUT http://localhost:3030/api/deadlines/{id}/complete`
   - **Input**:
     ```
     curl -s -X PUT "http://localhost:3030/api/deadlines/<deadline_id>/complete" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK with non-null `completed_at`

4. List deadlines without include_completed (default false)
   - **Target**: `GET http://localhost:3030/api/deadlines?days=30`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?days=30" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: Response does NOT contain the completed deadline's `id`

5. List deadlines with include_completed=true
   - **Target**: `GET http://localhost:3030/api/deadlines?days=30&include_completed=true`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?days=30&include_completed=true" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: Response DOES contain the completed deadline's `id` with a non-null `completed_at` field

6. List deadlines with include_completed=false (explicit)
   - **Target**: `GET http://localhost:3030/api/deadlines?days=30&include_completed=false`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?days=30&include_completed=false" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: Response does NOT contain the completed deadline — same behavior as step 4

## Success Criteria
- [ ] Default listing (no param) excludes completed deadlines
- [ ] `include_completed=true` includes completed deadlines alongside pending ones
- [ ] `include_completed=false` explicitly excludes completed deadlines (same as default)
- [ ] Completed deadlines in the `include_completed=true` response have a non-null `completed_at` timestamp
- [ ] Pending deadlines appear in all three variants of the listing

## Failure Criteria
- Completed deadlines appear in the default listing
- `include_completed=true` response does not include the completed deadline
- `completed_at` is null or absent on completed deadlines in the inclusive listing
- Pending deadlines disappear from any listing variant

## Notes
The `include_completed` parameter gates whether `WHERE completed_at IS NULL` is applied to the query. Default behavior (omitted or `false`) must filter out completed rows. This test also validates that `include_completed=false` behaves identically to the default, confirming the parameter is parsed correctly and not inverted.
