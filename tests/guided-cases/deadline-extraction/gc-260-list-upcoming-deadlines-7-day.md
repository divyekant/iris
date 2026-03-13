# GC-260: List Upcoming Deadlines Within 7-Day Window

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: deadline-extraction
- **Tags**: deadlines, list, upcoming, window, days-param
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- At least one deadline previously extracted (via `POST /api/ai/extract-deadlines`) with a `deadline_date` falling within the next 7 days from today (2026-03-13) (source: prior extraction run or setup step)
- At least one deadline with a `deadline_date` beyond 7 days, to confirm the window filter excludes it (source: prior extraction run)

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Ensure at least one deadline exists within the 7-day window by extracting from a suitable email
   - **Target**: `POST http://localhost:3030/api/ai/extract-deadlines`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/extract-deadlines \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "<message_id_with_near_deadline>"}'
     ```
   - **Expected**: 200 OK with at least one deadline returned

3. List upcoming deadlines with the default 7-day window
   - **Target**: `GET http://localhost:3030/api/deadlines?days=7`
   - **Input**:
     ```
     curl -s "http://localhost:3030/api/deadlines?days=7" \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: 200 OK with JSON body `{"deadlines": [...]}` containing deadlines with `deadline_date` within the next 7 days

4. Verify no deadlines beyond the window are included
   - **Target**: Response from step 3
   - **Input**: Inspect each `deadline_date` value
   - **Expected**: All returned deadlines have `deadline_date` between today (2026-03-13) and 2026-03-20 (inclusive); no deadlines with dates beyond 2026-03-20 appear

5. Confirm completed deadlines are excluded by default
   - **Target**: Response from step 3
   - **Input**: Inspect each deadline's `completed` or `completed_at` field
   - **Expected**: No completed deadlines appear in the list (default `include_completed=false`)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains `deadlines` array
- [ ] All returned deadlines have `deadline_date` within 7 days of today
- [ ] Deadlines beyond the 7-day window are not included
- [ ] Completed deadlines are absent from the default listing

## Failure Criteria
- Response status is not 200
- Deadlines outside the 7-day window appear in the response
- Completed deadlines appear in the response when `include_completed` is not set
- `deadlines` key is missing from the response body

## Notes
The `?days=7` parameter defines the lookahead window. Deadlines are expected to be filtered by `deadline_date <= NOW() + 7 days`. The `include_completed` query param defaults to `false`; completed deadlines should only appear when explicitly requested (tested in GC-268).
