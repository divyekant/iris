# GC-158: Concurrent Requests Do Not Interfere

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai, edge, concurrency, isolation, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- Two distinct email body strings prepared (body_A and body_B)

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Fire two suggest-subject requests concurrently (use parallel curl or similar)
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject` (two simultaneous calls)
   - **Input**:
     - Request A: `{"body": "Please find attached the quarterly financial report. Revenue is up 18% year-over-year."}`
     - Request B: `{"body": "Just a reminder that your dentist appointment is scheduled for Friday at 3pm."}`
   - **Expected**: Both requests return 200 independently, each with their own suggestions array; no response body bleed (A's response does not contain B's suggestions or vice versa)

3. Verify response isolation
   - **Target**: response bodies from step 2
   - **Input**: none
   - **Expected**: Suggestions in response A are topically related to financial reporting; suggestions in response B are topically related to appointment reminders — no cross-contamination of content

4. Repeat with 5 simultaneous requests using the same body to verify idempotent results
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject` (5 simultaneous calls)
   - **Input**: All 5 requests use `{"body": "Team lunch is confirmed for noon on Wednesday at the Italian place."}`
   - **Expected**: All 5 return 200 with suggestions arrays; server does not return 500 or 429 under this light concurrency load

## Success Criteria
- [ ] Both concurrent requests return 200
- [ ] Response A suggestions are not present in response B (no cross-contamination)
- [ ] Response B suggestions are not present in response A
- [ ] 5 simultaneous identical requests all return 200 without 500 errors
- [ ] No request hangs indefinitely (all respond within a reasonable timeout, e.g., 60s)

## Failure Criteria
- Either concurrent request returns 500
- Response body from one request contains suggestions clearly belonging to the other request's topic
- Any of the 5 simultaneous requests hangs without response
- Server returns 429 Too Many Requests under this minimal concurrency (5 requests is not high load)
