# GC-269: Compute VIP Scores — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, scoring, compute
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one synced account with message history (source: prior sync)
- Messages from at least one external contact exist in the database so the scoring algorithm has input data to work with

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Trigger VIP score computation
   - **Target**: `POST http://localhost:3000/api/contacts/vip/compute`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/contacts/vip/compute \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, response body contains `contacts_scored` and `vip_count` fields; `contacts_scored` is a non-negative integer

3. Verify response shape
   - **Target**: Response JSON inspection
   - **Input**: Parse the JSON returned in step 2
   - **Expected**: Both `contacts_scored` and `vip_count` are integers where `vip_count` <= `contacts_scored`

4. Confirm vip_contacts table was populated
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `vip_contacts` array contains contacts with `email`, `vip_score`, `is_manual`, `message_count`, `reply_count` fields

## Success Criteria
- [ ] POST /api/contacts/vip/compute returns 200 OK
- [ ] Response contains `contacts_scored` integer >= 0
- [ ] Response contains `vip_count` integer >= 0 and <= `contacts_scored`
- [ ] Subsequent GET /api/contacts/vip returns contacts that have `vip_score` populated (not null)
- [ ] Each contact in the VIP list has `email`, `vip_score`, `is_manual`, `message_count`, `reply_count` fields

## Failure Criteria
- POST returns non-200 status
- Response is missing `contacts_scored` or `vip_count` fields
- `vip_count` > `contacts_scored` (impossible by logic)
- Server returns 500 Internal Server Error

## Notes
The compute endpoint reads from `messages` table, scores all external contacts using the weighted algorithm (frequency 0.30, reply_rate 0.25, recency 0.25, thread_depth 0.20), and upserts into `vip_contacts`. It respects existing `is_manual=1` rows by keeping score at 1.0 rather than overwriting. The endpoint is idempotent — calling it multiple times with the same data should yield the same results.
