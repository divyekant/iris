# GC-632: Temporal Reasoning — GET /api/timeline Returns Extracted Timeline Events

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: temporal-reasoning, timeline, events, extraction
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least 2 messages have had timeline events extracted (e.g., "Meeting on March 20", "Deadline: March 25") via prior ingest or explicit extraction
- Timeline events exist in the `timeline_events` table

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Fetch the timeline
   - **Target**: `GET http://localhost:3030/api/timeline`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, response contains `events` array

3. Verify event structure
   - **Target**: `events` array from step 2
   - **Input**: inspect each event object
   - **Expected**: each event has `id`, `message_id`, `description`, `event_date` (ISO timestamp), `event_type` (e.g., `meeting`, `deadline`, `reminder`)

4. Verify chronological ordering
   - **Target**: `events` array from step 2
   - **Input**: compare consecutive `event_date` values
   - **Expected**: events are ordered by `event_date` ascending (soonest first)

## Success Criteria
- [ ] GET /api/timeline returns 200 OK
- [ ] `events` array is non-empty (given precondition data)
- [ ] Each event has `id`, `message_id`, `description`, `event_date`, `event_type`
- [ ] Events are sorted by `event_date` ascending
- [ ] `message_id` values reference real messages in the database

## Failure Criteria
- Empty `events` array when timeline data exists
- Events missing required fields
- Events not sorted by date
- `message_id` references non-existent messages
