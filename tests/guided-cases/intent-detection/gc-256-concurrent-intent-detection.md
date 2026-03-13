# GC-256: Concurrent Intent Detection Requests

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, concurrency, isolation, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- Three distinct message IDs from the synced inbox (source: `GET /api/messages`); each message should have clearly different content to allow cross-contamination detection:
  - `$MSG_A`: a message about a meeting scheduling request
  - `$MSG_B`: a newsletter from a marketing list
  - `$MSG_C`: a direct question about a project status

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Fire 3 concurrent detect-intent requests for different messages
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent` (3 parallel requests)
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" -H "Content-Type: application/json" \
       -d '{"message_id": "$MSG_A"}' &

     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" -H "Content-Type: application/json" \
       -d '{"message_id": "$MSG_B"}' &

     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" -H "Content-Type: application/json" \
       -d '{"message_id": "$MSG_C"}' &

     wait
     ```
   - **Expected**: All 3 requests complete with 200 OK; each returns an `intent` and `confidence` for its respective message

3. Verify response isolation — A returns `scheduling`, B returns `newsletter`, C returns `question`
   - **Target**: Response bodies from step 2
   - **Input**: n/a
   - **Expected**: Each response's `intent` is plausible for its message content; no cross-contamination (e.g., MSG_B does not return `scheduling` when its content is clearly newsletter)

4. Fire 5 concurrent detect-intent requests for the same message ID
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent` (5 parallel requests, all with `$MSG_A`)
   - **Input**:
     ```
     for i in 1 2 3 4 5; do
       curl -s -X POST http://localhost:3000/api/ai/detect-intent \
         -H "x-session-token: $TOKEN" -H "Content-Type: application/json" \
         -d '{"message_id": "$MSG_A"}' &
     done
     wait
     ```
   - **Expected**: All 5 return 200; all return the same or equivalent `intent` value (same message, same expected classification); no 500 errors; server does not deadlock or timeout

5. Verify server health after concurrent load
   - **Target**: `GET http://localhost:3000/api/health`
   - **Input**: n/a
   - **Expected**: 200 OK; server is stable and responsive

## Success Criteria
- [ ] All 3 distinct-message concurrent requests return 200
- [ ] Each response's `intent` is plausible for its own message (no response bleed)
- [ ] All 5 same-message concurrent requests return 200 without 500 errors
- [ ] All 5 same-message responses return the same or equivalent `intent` value
- [ ] Server health check passes after all concurrent requests

## Failure Criteria
- Any concurrent request returns 500 or hangs without response within 60 seconds
- A response body contains the `intent` of a different message (cross-contamination)
- Server health endpoint returns non-200 after the concurrent load
- Server returns 429 Too Many Requests (5 concurrent is minimal load, not rate-limit territory)
